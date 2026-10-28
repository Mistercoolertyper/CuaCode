//! The headless render harness: a frame of this app, drawn off-screen, written to a
//! PNG. **Nothing appears on the display.**
//!
//! GPUI's headless context opens the window through `TestPlatform` over a headless GPU
//! renderer and reads the rendered pixels back, so a frame can be looked at without
//! taking over the machine it is being looked at on. That is the whole point of it, and
//! the reason it exists rather than a launch and a screen grab.
//!
//! It renders the **real** `App` - the same window, the same state root, the same
//! composer - not a stand-in, so what comes out is what a person would see.
//!
//! ## Why this shape
//!
//! Three constraints, and each one moved the file:
//!
//! - `HeadlessAppContext` and `current_headless_renderer` are behind GPUI's
//!   `test-support` feature. A *library* feature cannot turn that on: enabling `shot`
//!   on this crate would not enable `gpui-kit/test-support` for the `gpui-pre` that is
//!   already compiled. The build that has it is the test profile, where this crate's
//!   dev-dependency asks for it.
//! - `MacPlatform::new` asserts it is on the **main thread**, and libtest runs a
//!   `#[test]` on a thread it spawned. So this is `harness = false` with its own `main`,
//!   which puts the whole thing back on the main thread.
//! - A `harness = false` integration test can only see the crate's **library**. Hence
//!   the `[lib]` and `[bin]` targets on one path in `Cargo.toml`.
//!
//! ```text
//! CUACODE_SHOT=/tmp/app.png cargo test -p cuacode-app --test shot
//! ```

use std::sync::Arc;

use cuacode_app::{App, Brands, ChainAssets};
// Named, not globbed: `use gpui_kit::*` re-exports gpui's own `test` macro.
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions, px, size,
    test::TestWindowExt as _,
};

fn main() {
    // A `harness = false` test runs on **every** `cargo test`, and this one would build a
    // GPU context and write a file each time. It is a tool, so it runs only when it is
    // asked for: `CUACODE_SHOT=<path> cargo test -p cuacode-app --test shot`.
    let Ok(out) = std::env::var("CUACODE_SHOT") else {
        eprintln!("shot: set CUACODE_SHOT=<png> to render a frame; nothing done");
        return;
    };
    let (w, h) = parse_size(&std::env::var("CUACODE_SHOT_SIZE").unwrap_or_else(|_| "1280x820".into()));
    let worker = std::env::var("CUACODE_WORKER").map(std::path::PathBuf::from).unwrap_or_default();

    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(ChainAssets::new(Brands, gpui_kit::assets::AllAssets::new(""))),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    // The same bar the app draws: one palette, one thumb tone.
    cx.update(cuacode_app::dress);

    let (handle, _) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(w), px(h)),
                    })),
                    // Off-screen by construction: `TestPlatform` never puts a window on
                    // the display, and `focus: false` keeps it from stealing any.
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| App::new(None, worker.clone(), window, cx)),
            )
        })
        .expect("open the offscreen window");

    // One frame, on demand. The app's own tick is a spawned task and does not run under
    // `TestPlatform` unless the clock is advanced, which is what is wanted: one frame.
    //
    // `CUACODE_SHOT_FRAMES=N` renders N frames and reports the **worst** one against
    // the selected session's real transcript. That is the number the handoff asked for
    // and never got: the render path is supposed to be disk-free and cheap, and a slow
    // frame here is a frame the pointer feels. It costs a second to run and it is the
    // only way to say "the lag is gone" without guessing.
    let frames: u32 = std::env::var("CUACODE_SHOT_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    for _ in 0..frames {
        let t = std::time::Instant::now();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .expect("render a frame");
        let dt = t.elapsed();
        worst = worst.max(dt);
        total += dt;
    }
    if frames > 1 {
        let mean = total / frames;
        println!(
            "frames: {frames}  mean {:.2}ms  worst {:.2}ms",
            mean.as_secs_f64() * 1000.0,
            worst.as_secs_f64() * 1000.0
        );
    }

    let image = cx
        .capture_screenshot(handle)
        .expect("read the rendered frame back");
    write_png(&out, image.width(), image.height(), image.as_raw());
    println!("shot: {out} ({}x{})", image.width(), image.height());
}

/// `WxH`, with a fallback rather than a panic: a bad size is a typo, not a crash.
fn parse_size(s: &str) -> (f32, f32) {
    let (w, h) = s.split_once('x').unwrap_or(("1280", "820"));
    (w.trim().parse().unwrap_or(1280.0), h.trim().parse().unwrap_or(820.0))
}

/// Write the frame as a PNG: the one place this touches the disk, and it writes one
/// file where it was told to. The pixels arrive as raw bytes rather than as the
/// renderer's own image type, so this does not have to name a crate that GPUI only
/// depends on transitively.
fn write_png(path: &str, w: u32, h: u32, raw: &[u8]) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let file = std::fs::File::create(path).unwrap_or_else(|e| panic!("create {path}: {e}"));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(raw).expect("png data");
}
