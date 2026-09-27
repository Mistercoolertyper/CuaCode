package main

// Inline formatting for the model's prose: a deliberately small subset of
// markdown - fenced code, inline code, bold, italic, headings, bullets,
// quotes, tables.
//
// Everything is parsed into styled spans and wrapped by the width of the text
// alone, so styling can never move a column. That is the whole reason this
// exists rather than colouring strings after wrapping them: the markers are
// gone by the time anything is measured.

import "strings"

// span is a run of text under one style.
type span struct {
	text  string
	style string
}

// atom is a wrappable unit: one word, plus whether a space separates it from
// the previous one.
type atom struct {
	text  string
	style string
	space bool
}

// formatProse turns a block of prose into wrapped, styled lines.
func formatProse(text string, w int) [][]span {
	if w < 4 {
		w = 4
	}

	var out [][]span
	code := false
	lines := strings.Split(text, "\n")

	for i := 0; i < len(lines); i++ {
		line := lines[i]
		if strings.HasPrefix(strings.TrimSpace(line), "```") {
			code = !code
			continue
		}

		if code {
			// Code keeps its own whitespace, so it is truncated rather than
			// wrapped - a re-flowed line of code is a lie about the code.
			out = append(out, []span{{text: trunc(" "+line, w), style: sCode}})
			continue
		}

		// A table is the one block shape that is not prose: its lines have to
		// become columns or the pipes are just noise. One that will not fit
		// falls through to the ordinary path rather than overflowing.
		if rows, align, next, is := tableAt(lines, i); is {
			if laid, fits := tableSpans(rows, align, w); fits {
				out = append(out, laid...)
				i = next - 1
				continue
			}
		}

		if strings.TrimSpace(line) == "" {
			out = append(out, nil)
			continue
		}

		body, prefix, hang, base := blockLead(line)
		lead := 0
		if prefix != nil {
			lead = vw(prefix.text)
		}
		wrapped := wrapAtoms(splitAtoms(parseInline(body)), w, lead, hang)
		for i, l := range wrapped {
			if base != "" {
				for j := range l {
					if l[j].style == "" {
						l[j].style = base
					}
				}
			}
			switch {
			case i == 0 && prefix != nil:
				out = append(out, append([]span{*prefix}, l...))
			case hang > 0:
				out = append(out, append([]span{{text: strings.Repeat(" ", hang)}}, l...))
			default:
				out = append(out, l)
			}
		}
	}
	return out
}

// blockLead strips a line's block marker, returning the remaining text, the
// span that replaces the marker, the indent its continuation lines get, and a
// style for the whole line.
func blockLead(line string) (body string, prefix *span, hang int, base string) {
	trimmed := strings.TrimLeft(line, " ")
	indent := len(line) - len(trimmed)

	switch {
	case strings.HasPrefix(trimmed, "#"):
		if h := strings.TrimLeft(trimmed, "#"); strings.HasPrefix(h, " ") {
			return strings.TrimSpace(h), nil, 0, sHead
		}

	case strings.HasPrefix(trimmed, "> "):
		return trimmed[2:], &span{text: "│ ", style: cFaint}, 2, cMuted

	case strings.HasPrefix(trimmed, "- "), strings.HasPrefix(trimmed, "* "), strings.HasPrefix(trimmed, "+ "):
		pad := strings.Repeat(" ", indent)
		return trimmed[2:], &span{text: pad + "• ", style: cFaint}, indent + 2, ""
	}
	return line, nil, 0, ""
}

// parseInline splits a line on `code`, **bold** and *italic*. An unclosed
// marker is left as literal text - half-typed emphasis is normal while a
// response is still streaming, and it must not restyle the rest of the line.
func parseInline(s string) []span {
	var (
		out  []span
		buf  strings.Builder
		base = ""
	)
	flush := func() {
		if buf.Len() > 0 {
			out = append(out, span{text: buf.String(), style: base})
			buf.Reset()
		}
	}

	for i := 0; i < len(s); {
		switch {
		case s[i] == '`':
			if j := strings.IndexByte(s[i+1:], '`'); j >= 0 {
				flush()
				out = append(out, span{text: s[i+1 : i+1+j], style: sCode})
				i += j + 2
				continue
			}

		case strings.HasPrefix(s[i:], "**"):
			if j := strings.Index(s[i+2:], "**"); j >= 0 {
				flush()
				out = append(out, span{text: s[i+2 : i+2+j], style: sBold})
				i += j + 4
				continue
			}

		case s[i] == '*':
			if j := strings.IndexByte(s[i+1:], '*'); j > 0 {
				flush()
				out = append(out, span{text: s[i+1 : i+1+j], style: sItalic})
				i += j + 2
				continue
			}
		}
		buf.WriteByte(s[i])
		i++
	}

	flush()
	return out
}

// splitAtoms breaks spans at spaces so the wrapper has something to break on.
func splitAtoms(spans []span) []atom {
	var out []atom
	pending := false

	for _, sp := range spans {
		for i, word := range strings.Split(sp.text, " ") {
			if i > 0 {
				pending = true
			}
			if word == "" {
				continue
			}
			out = append(out, atom{text: word, style: sp.style, space: pending && len(out) > 0})
			pending = false
		}
	}
	return out
}

// wrapAtoms greedily fills lines of width w. The first line gives up lead cells
// to the marker that will be prepended to it; every line after gives up hang to
// the indent that keeps it under the first.
func wrapAtoms(atoms []atom, w, lead, hang int) [][]span {
	var (
		out  [][]span
		line []span
		used int
	)
	limit := func() int {
		if len(out) == 0 {
			return max(w-lead, 1)
		}
		return max(w-hang, 1)
	}

	for _, a := range atoms {
		word := a.text
		for {
			gap := 0
			if a.space && used > 0 {
				gap = 1
			}
			if room := limit() - used - gap; vw(word) > room {
				if used > 0 {
					out, line, used = append(out, line), nil, 0
					a.space = false
					continue
				}
				// A single word longer than the line: split it rather than
				// overflow, because overflow is what breaks the frame.
				head := trimToWidth(word, limit())
				out = append(out, []span{{text: head, style: a.style}})
				word = word[len(head):]
				if word == "" {
					break
				}
				continue
			}
			if gap > 0 {
				line = append(line, span{text: " "})
				used++
			}
			line = append(line, span{text: word, style: a.style})
			used += vw(word)
			break
		}
	}

	if len(line) > 0 {
		out = append(out, line)
	}
	return out
}

// trimToWidth returns the longest prefix of s that fits in w cells.
func trimToWidth(s string, w int) string {
	if w < 1 {
		w = 1
	}
	used, end := 0, 0
	for i, r := range s {
		rw := vw(string(r))
		if used+rw > w {
			break
		}
		used, end = used+rw, i+len(string(r))
	}
	if end == 0 {
		_, size := decodeRune(s)
		end = size
	}
	return s[:end]
}

func decodeRune(s string) (rune, int) {
	for i, r := range s {
		_ = i
		return r, len(string(r))
	}
	return 0, 0
}

// spanWidth is the visible width of a line of spans.
func spanWidth(line []span) int {
	w := 0
	for _, sp := range line {
		w += vw(sp.text)
	}
	return w
}

// renderSpans paints a line, falling back to base wherever a span has no style
// of its own.
func renderSpans(line []span, base string) string {
	var b strings.Builder
	for _, sp := range line {
		style := sp.style
		if style == "" {
			style = base
		}
		b.WriteString(paint(style, sp.text))
	}
	return b.String()
}

// Tables.
//
// A table is the one thing an agent writes that prose rendering cannot read:
// pipes and dashes are not a sentence, they are a grid. Cells get the same
// inline formatting any other run of the model's text does, and are then laid
// out in columns sized to their contents.

const (
	tableGap    = 2 // cells between columns, where the rule has its posts
	tableMinCol = 4 // narrowest a column may be squeezed to before giving up
)

// Column alignments, taken from the colons of the rule row.
const (
	alignLeft = iota
	alignRight
	alignCenter
)

// tableAt reads the markdown table beginning at lines[i]. A header row followed
// by a |---|---| rule is the whole test: nothing else in prose has that shape,
// so an inline a | b in a sentence stays a sentence, and a table still streaming
// in - header written, rule not yet arrived - stays prose until it is a table.
func tableAt(lines []string, i int) (rows [][]string, align []int, next int, ok bool) {
	if i+1 >= len(lines) || !strings.Contains(lines[i], "|") {
		return nil, nil, 0, false
	}
	head, rule := tableCells(lines[i]), tableCells(lines[i+1])
	if len(head) < 2 || len(rule) < 2 || !dashes(rule) {
		return nil, nil, 0, false
	}

	cols := len(head)
	align = make([]int, cols)
	for c := range align {
		if c < len(rule) && strings.HasSuffix(rule[c], ":") {
			align[c] = alignRight
			if strings.HasPrefix(rule[c], ":") {
				align[c] = alignCenter
			}
		}
	}

	rows = [][]string{head}
	for j := i + 2; j < len(lines); j++ {
		if !strings.Contains(lines[j], "|") || strings.TrimSpace(lines[j]) == "" {
			return rows, align, j, true
		}
		cells := tableCells(lines[j])
		for len(cells) < cols {
			cells = append(cells, "")
		}
		rows = append(rows, cells[:cols])
	}
	return rows, align, len(lines), true
}

// tableCells splits a row on its pipes. Markdown makes the leading and trailing
// pipe optional, so both come off before the split.
func tableCells(line string) []string {
	cells := strings.Split(strings.Trim(strings.TrimSpace(line), "|"), "|")
	for i := range cells {
		cells[i] = strings.TrimSpace(cells[i])
	}
	return cells
}

// dashes reports whether every cell is the ---: of the rule row.
func dashes(cells []string) bool {
	for _, c := range cells {
		dashes := strings.Trim(c, ":")
		if dashes == "" || strings.Trim(dashes, "-") != "" {
			return false
		}
	}
	return true
}

// tableSpans lays a table out inside w cells, or reports false when it cannot
// be drawn: a table squeezed past the point of columns is more readable as the
// lines the model wrote.
func tableSpans(rows [][]string, align []int, w int) ([][]span, bool) {
	cols := len(rows[0])
	budget := w - tableGap*(cols-1)
	if budget < cols*tableMinCol {
		return nil, false
	}

	// Cells are parsed first, so the markers are gone by the time anything is
	// measured - the same reason the rest of this file parses before it wraps.
	cells := make([][][]span, len(rows))
	natural := make([]int, cols)
	for r, row := range rows {
		cells[r] = make([][]span, cols)
		for c, cell := range row {
			cells[r][c] = parseInline(cell)
			natural[c] = max(natural[c], spanWidth(cells[r][c]))
		}
	}

	width := fitColumns(natural, budget)
	if width == nil {
		return nil, false
	}

	var out [][]span
	for r := range rows {
		// The header names the columns, so it is the row that carries weight.
		style := ""
		if r == 0 {
			style = sBold
		}
		out = append(out, tableRow(cells[r], width, align, style)...)
		if r == 0 {
			out = append(out, tableRule(width))
		}
	}
	return out, true
}

// fitColumns shrinks the natural widths until they fit the budget, a cell at a
// time off whichever column is currently widest, so wide columns give way
// together instead of one of them starving. nil when even the minimum is too
// wide for the room there is.
func fitColumns(natural []int, budget int) []int {
	width := append([]int(nil), natural...)
	total := 0
	for _, n := range width {
		total += n
	}
	for total > budget {
		at, most := -1, tableMinCol
		for c, n := range width {
			if n > most {
				most, at = n, c
			}
		}
		if at < 0 {
			return nil
		}
		width[at]--
		total--
	}
	return width
}

// tableRow lays one row out as one or more lines: a cell that does not fit its
// column wraps inside it, and the row grows to its tallest cell.
func tableRow(cells [][]span, width, align []int, style string) [][]span {
	wrapped := make([][][]span, len(cells))
	height := 1
	for c, cell := range cells {
		wrapped[c] = wrapAtoms(splitAtoms(cell), width[c], 0, 0)
		height = max(height, len(wrapped[c]))
	}

	out := make([][]span, 0, height)
	for i := 0; i < height; i++ {
		var row []span
		for c := range cells {
			cell := []span(nil)
			if i < len(wrapped[c]) {
				cell = wrapped[c][i]
			}
			for j := range cell {
				if cell[j].style == "" {
					cell[j].style = style
				}
			}

			// Alignment is slack: whatever the cell leaves of its column is put
			// in front of it, behind it, or split between the two.
			slack := max(width[c]-spanWidth(cell), 0)
			left, right := 0, slack
			switch align[c] {
			case alignRight:
				left, right = slack, 0
			case alignCenter:
				left, right = slack/2, slack-slack/2
			}

			if left > 0 {
				row = append(row, space(left))
			}
			row = append(row, cell...)
			row = append(row, space(right+tableGap))
		}
		out = append(out, trimTail(row))
	}
	return out
}

// tableRule is the quiet line under the header: the rule a real table draws,
// column for column.
func tableRule(width []int) []span {
	var row []span
	for c, w := range width {
		if c > 0 {
			row = append(row, space(tableGap))
		}
		row = append(row, span{text: strings.Repeat("─", w), style: cFaint})
	}
	return row
}

func space(n int) span { return span{text: strings.Repeat(" ", n)} }

// trimTail drops the padding off the end of a row: selecting a table in the
// terminal should copy the table, not the rectangle to the right of it.
func trimTail(line []span) []span {
	for len(line) > 0 {
		last := len(line) - 1
		line[last].text = strings.TrimRight(line[last].text, " ")
		if line[last].text != "" {
			break
		}
		line = line[:last]
	}
	return line
}
