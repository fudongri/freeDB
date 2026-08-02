//! Text cursor changes/interaction, without modifying the text.

use epaint::text::{Galley, cursor::CCursor};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{NumExt as _, Rect, Response, Ui, epaint};

use super::CCursorRange;

/// The state of a text cursor selection.
///
/// Used for [`crate::TextEdit`] and [`crate::Label`].
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct TextCursorState {
    ccursor_range: Option<CCursorRange>,
}

impl From<CCursorRange> for TextCursorState {
    fn from(ccursor_range: CCursorRange) -> Self {
        Self {
            ccursor_range: Some(ccursor_range),
        }
    }
}

impl TextCursorState {
    pub fn is_empty(&self) -> bool {
        self.ccursor_range.is_none()
    }

    /// The currently selected range of characters.
    pub fn char_range(&self) -> Option<CCursorRange> {
        self.ccursor_range
    }

    /// The currently selected range of characters, clamped within the character
    /// range of the given [`Galley`].
    pub fn range(&self, galley: &Galley) -> Option<CCursorRange> {
        self.ccursor_range.map(|mut range| {
            range.primary = galley.clamp_cursor(&range.primary);
            range.secondary = galley.clamp_cursor(&range.secondary);
            range
        })
    }

    /// Sets the currently selected range of characters.
    pub fn set_char_range(&mut self, ccursor_range: Option<CCursorRange>) {
        self.ccursor_range = ccursor_range;
    }
}

impl TextCursorState {
    /// Handle clicking and/or dragging text.
    ///
    /// Returns `true` if there was interaction.
    pub fn pointer_interaction(
        &mut self,
        ui: &Ui,
        response: &Response,
        cursor_at_pointer: CCursor,
        galley: &Galley,
        is_being_dragged: bool,
    ) -> bool {
        let text = galley.text();

        if response.double_clicked() {
            // Select word:
            let ccursor_range = select_word_at(text, cursor_at_pointer);
            self.set_char_range(Some(ccursor_range));
            true
        } else if response.triple_clicked() {
            // Select line:
            let ccursor_range = select_line_at(text, cursor_at_pointer);
            self.set_char_range(Some(ccursor_range));
            true
        } else if response.sense.senses_drag() {
            if response.hovered() && ui.input(|i| i.pointer.any_pressed() && !i.pointer.secondary_pressed()) {
                // The start of a drag (or a click).
                // Secondary button (right-click) opens context menus — don't touch the selection.
                if ui.input(|i| i.modifiers.shift) {
                    if let Some(mut cursor_range) = self.range(galley) {
                        cursor_range.primary = cursor_at_pointer;
                        self.set_char_range(Some(cursor_range));
                    } else {
                        self.set_char_range(Some(CCursorRange::one(cursor_at_pointer)));
                    }
                } else {
                    self.set_char_range(Some(CCursorRange::one(cursor_at_pointer)));
                }
                true
            } else if is_being_dragged {
                // Drag to select text:
                if let Some(mut cursor_range) = self.range(galley) {
                    cursor_range.primary = cursor_at_pointer;
                    self.set_char_range(Some(cursor_range));
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

fn select_word_at(text: &str, ccursor: CCursor) -> CCursorRange {
    if ccursor.index == 0 {
        CCursorRange::two(ccursor, ccursor_next_word(text, ccursor))
    } else {
        let it = text.chars();
        let mut it = it.skip(ccursor.index - 1);
        if let Some(char_before_cursor) = it.next() {
            if let Some(char_after_cursor) = it.next() {
                if is_word_char(char_before_cursor) && is_word_char(char_after_cursor) {
                    let min = ccursor_previous_word(text, ccursor + 1);
                    let max = ccursor_next_word(text, min);
                    CCursorRange::two(min, max)
                } else if is_word_char(char_before_cursor) {
                    let min = ccursor_previous_word(text, ccursor);
                    let max = ccursor_next_word(text, min);
                    CCursorRange::two(min, max)
                } else if is_word_char(char_after_cursor) {
                    let max = ccursor_next_word(text, ccursor);
                    CCursorRange::two(ccursor, max)
                } else {
                    let min = ccursor_previous_word(text, ccursor);
                    let max = ccursor_next_word(text, ccursor);
                    // 双击落在非词字符间隙（换行/空白/标点）时，原逻辑把上一词起点到下一词
                    // 终点全部选中，会把换行符连同邻词一起跨行选中。双击选词不应跨行：
                    // 跳过区间开头的换行符，并把终点收缩到第一个换行符之前。
                    let chars: Vec<char> = text.chars().collect();
                    let mut sel_start = min.index.min(chars.len());
                    let mut sel_end = max.index.min(chars.len());
                    while sel_start < sel_end && matches!(chars[sel_start], '\n' | '\r') {
                        sel_start += 1;
                    }
                    if let Some(rel) = chars[sel_start..sel_end]
                        .iter()
                        .position(|&c| c == '\n' || c == '\r')
                    {
                        sel_end = sel_start + rel;
                    }
                    CCursorRange::two(CCursor::new(sel_start), CCursor::new(sel_end))
                }
            } else {
                let min = ccursor_previous_word(text, ccursor);
                CCursorRange::two(min, ccursor)
            }
        } else {
            let max = ccursor_next_word(text, ccursor);
            CCursorRange::two(ccursor, max)
        }
    }
}

fn select_line_at(text: &str, ccursor: CCursor) -> CCursorRange {
    if ccursor.index == 0 {
        CCursorRange::two(ccursor, ccursor_next_line(text, ccursor))
    } else {
        let it = text.chars();
        let mut it = it.skip(ccursor.index - 1);
        if let Some(char_before_cursor) = it.next() {
            if let Some(char_after_cursor) = it.next() {
                if (!is_linebreak(char_before_cursor)) && (!is_linebreak(char_after_cursor)) {
                    let min = ccursor_previous_line(text, ccursor + 1);
                    let max = ccursor_next_line(text, min);
                    CCursorRange::two(min, max)
                } else if !is_linebreak(char_before_cursor) {
                    let min = ccursor_previous_line(text, ccursor);
                    let max = ccursor_next_line(text, min);
                    CCursorRange::two(min, max)
                } else if !is_linebreak(char_after_cursor) {
                    let max = ccursor_next_line(text, ccursor);
                    CCursorRange::two(ccursor, max)
                } else {
                    let min = ccursor_previous_line(text, ccursor);
                    let max = ccursor_next_line(text, ccursor);
                    CCursorRange::two(min, max)
                }
            } else {
                let min = ccursor_previous_line(text, ccursor);
                CCursorRange::two(min, ccursor)
            }
        } else {
            let max = ccursor_next_line(text, ccursor);
            CCursorRange::two(ccursor, max)
        }
    }
}

pub fn ccursor_next_word(text: &str, ccursor: CCursor) -> CCursor {
    CCursor {
        index: next_word_boundary_char_index(text, ccursor.index),
        prefer_next_row: false,
    }
}

fn ccursor_next_line(text: &str, ccursor: CCursor) -> CCursor {
    CCursor {
        index: next_line_boundary_char_index(text.chars(), ccursor.index),
        prefer_next_row: false,
    }
}

pub fn ccursor_previous_word(text: &str, ccursor: CCursor) -> CCursor {
    let num_chars = text.chars().count();
    let reversed: String = text.graphemes(true).rev().collect();
    CCursor {
        index: num_chars
            - next_word_boundary_char_index(&reversed, num_chars - ccursor.index).min(num_chars),
        prefer_next_row: true,
    }
}

fn ccursor_previous_line(text: &str, ccursor: CCursor) -> CCursor {
    let num_chars = text.chars().count();
    CCursor {
        index: num_chars
            - next_line_boundary_char_index(text.chars().rev(), num_chars - ccursor.index),
        prefer_next_row: true,
    }
}

fn next_word_boundary_char_index(text: &str, cursor_ci: usize) -> usize {
    let mut current_char_idx = 0;

    for (_word_byte_index, word) in text.split_word_bound_indices() {
        let word_ci = current_char_idx;

        // We consider `.` a word boundary.
        // At least that's how Mac works when navigating something like `www.example.com`.
        let mut word_char_count = 0;
        for chr in word.chars() {
            let dot_ci = word_ci + word_char_count;
            if chr == '.' && cursor_ci < dot_ci {
                return dot_ci;
            }
            word_char_count += 1;
        }

        // Splitting considers contiguous whitespace as one word, such words must be skipped,
        // this handles cases for example ' abc' (a space and a word), the cursor is at the beginning
        // (before space) - this jumps at the end of 'abc' (this is consistent with text editors
        // or browsers)
        if cursor_ci < word_ci && !all_word_chars(word) {
            return word_ci;
        }

        current_char_idx += word_char_count;
    }

    current_char_idx
}

fn all_word_chars(text: &str) -> bool {
    text.chars().all(is_word_char)
}

fn next_line_boundary_char_index(it: impl Iterator<Item = char>, mut index: usize) -> usize {
    let mut it = it.skip(index);
    if let Some(_first) = it.next() {
        index += 1;

        if let Some(second) = it.next() {
            index += 1;
            for next in it {
                if is_linebreak(next) != is_linebreak(second) {
                    break;
                }
                index += 1;
            }
        }
    }
    index
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_linebreak(c: char) -> bool {
    c == '\r' || c == '\n'
}

/// Accepts and returns character offset (NOT byte offset!).
pub fn find_line_start(text: &str, current_index: CCursor) -> CCursor {
    // We know that new lines, '\n', are a single byte char, but we have to
    // work with char offsets because before the new line there may be any
    // number of multi byte chars.
    // We need to know the char index to be able to correctly set the cursor
    // later.
    let chars_count = text.chars().count();

    let position = text
        .chars()
        .rev()
        .skip(chars_count - current_index.index)
        .position(|x| x == '\n');

    match position {
        Some(pos) => CCursor::new(current_index.index - pos),
        None => CCursor::new(0),
    }
}

pub fn byte_index_from_char_index(s: &str, char_index: usize) -> usize {
    for (ci, (bi, _)) in s.char_indices().enumerate() {
        if ci == char_index {
            return bi;
        }
    }
    s.len()
}

pub fn char_index_from_byte_index(input: &str, byte_index: usize) -> usize {
    for (ci, (bi, _)) in input.char_indices().enumerate() {
        if bi == byte_index {
            return ci;
        }
    }

    input.char_indices().last().map_or(0, |(i, _)| i + 1)
}

pub fn slice_char_range(s: &str, char_range: std::ops::Range<usize>) -> &str {
    assert!(
        char_range.start <= char_range.end,
        "Invalid range, start must be less than end, but start = {}, end = {}",
        char_range.start,
        char_range.end
    );
    let start_byte = byte_index_from_char_index(s, char_range.start);
    let end_byte = byte_index_from_char_index(s, char_range.end);
    &s[start_byte..end_byte]
}

/// The thin rectangle of one end of the selection, e.g. the primary cursor, in local galley coordinates.
pub fn cursor_rect(galley: &Galley, cursor: &CCursor, row_height: f32) -> Rect {
    let mut cursor_pos = galley.pos_from_cursor(*cursor);

    // Handle completely empty galleys
    cursor_pos.max.y = cursor_pos.max.y.at_least(cursor_pos.min.y + row_height);

    cursor_pos = cursor_pos.expand(1.5); // slightly above/below row

    cursor_pos
}

#[cfg(test)]
mod test {
    use crate::text_selection::text_cursor_state::next_word_boundary_char_index;
    use crate::text_selection::text_cursor_state::select_word_at;
    use epaint::text::cursor::CCursor;

    #[test]
    fn test_next_word_boundary_char_index() {
        // ASCII only
        let text = "abc d3f g_h i-j";
        assert_eq!(next_word_boundary_char_index(text, 1), 3);
        assert_eq!(next_word_boundary_char_index(text, 3), 7);
        assert_eq!(next_word_boundary_char_index(text, 9), 11);
        assert_eq!(next_word_boundary_char_index(text, 12), 13);
        assert_eq!(next_word_boundary_char_index(text, 13), 15);
        assert_eq!(next_word_boundary_char_index(text, 15), 15);

        assert_eq!(next_word_boundary_char_index("", 0), 0);
        assert_eq!(next_word_boundary_char_index("", 1), 0);

        // ASCII only
        let text = "abc.def.ghi";
        assert_eq!(next_word_boundary_char_index(text, 1), 3);
        assert_eq!(next_word_boundary_char_index(text, 3), 7);
        assert_eq!(next_word_boundary_char_index(text, 7), 11);

        // Unicode graphemes, some of which consist of multiple Unicode characters,
        // !!! Unicode character is not always what is tranditionally considered a character,
        // the values below are correct despite not seeming that way on the first look,
        // handling of and around emojis is kind of weird and is not consistent across
        // text editors and browsers
        let text = "❤️👍 skvělá knihovna 👍❤️";
        assert_eq!(next_word_boundary_char_index(text, 0), 2);
        assert_eq!(next_word_boundary_char_index(text, 2), 3); // this does not skip the space between thumbs-up and 'skvělá'
        assert_eq!(next_word_boundary_char_index(text, 6), 10);
        assert_eq!(next_word_boundary_char_index(text, 9), 10);
        assert_eq!(next_word_boundary_char_index(text, 12), 19);
        assert_eq!(next_word_boundary_char_index(text, 15), 19);
        assert_eq!(next_word_boundary_char_index(text, 19), 20);
        assert_eq!(next_word_boundary_char_index(text, 20), 21);
    }

    /// 双击选词：双击落在非词字符间隙（含换行符）时不得跨行选中。
    fn range_of(text: &str, ccursor_index: usize) -> (usize, usize) {
        let range = select_word_at(text, CCursor::new(ccursor_index)).as_sorted_char_range();
        let selected: String = text.chars().skip(range.start).take(range.end - range.start).collect();
        assert!(
            !selected.contains('\n') && !selected.contains('\r'),
            "双击选词不得包含换行符, 选中了 {:?}",
            selected
        );
        (range.start, range.end)
    }

    #[test]
    fn double_click_at_line_end_trailing_space_does_not_cross_line() {
        // 行尾尾随空格：双击落在空格与换行之间，原逻辑会选中 "foo \nFROM"
        let text = "SELECT foo \nFROM bar";
        // 光标在 "foo " 与换行之间（idx 11 = 空格后）
        assert_eq!(range_of(text, 11), (7, 11));
        // 普通单词双击不受影响
        assert_eq!(range_of(text, 8), (7, 10));
        assert_eq!(range_of(text, 12), (12, 16));
    }

    #[test]
    fn double_click_on_empty_line_does_not_cross_line() {
        // 空行：双击落在两个换行符之间，原逻辑会选中 "foo\n\nFROM"
        let text = "SELECT foo\n\nFROM bar";
        assert_eq!(range_of(text, 11), (7, 10));
        assert_eq!(range_of(text, 10), (7, 10));
        assert_eq!(range_of(text, 12), (12, 16));
    }

    #[test]
    fn double_click_on_folded_ellipsis_does_not_cross_line() {
        // 折叠 display 文本：双击落在 … 占位符相邻间隙，原逻辑会跨行选中
        let text = "f(\n…\n)\nbaz";
        // 双击在 … 前（换行与 … 之间，idx 3）：跳过换行只选中 …
        assert_eq!(range_of(text, 3), (3, 4));
        // 双击在 … 与换行之间（idx 4）：同样只选中 …
        assert_eq!(range_of(text, 4), (3, 4));
        // 双击在 ) 与换行之间（idx 6）：只选中 )
        assert_eq!(range_of(text, 6), (5, 6));
        // 双击在 ( 与换行之间（idx 2）：裁剪到行尾只选中 "f("
        assert_eq!(range_of(text, 2), (0, 2));
        // 普通单词双击不受影响
        assert_eq!(range_of(text, 8), (7, 10));
    }
}
