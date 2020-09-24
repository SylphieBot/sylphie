//! Handles parsing command arguments from a raw string.

use derive_setters::*;

/// Defines how arguments are parsed for the context type.
#[derive(Copy, Clone, Debug, Default, Setters)]
#[non_exhaustive]
pub struct ArgParsingOptions {
    /// Whether to parse the input as markdown.
    #[setters(bool)]
    pub parse_markdown: bool,
}

#[derive(Clone, Debug)]
enum ArgsType {
    Empty,
    Span(usize, usize),
    Inline(String),
}
impl ArgsType {
    fn as_str<'a: 'c, 'b: 'c, 'c>(&'a self, source: &'b str) -> &'c str {
        match &self {
            ArgsType::Empty => "",
            ArgsType::Span(start, end) => &source[*start..*end],
            ArgsType::Inline(s) => s,
        }
    }
}

#[derive(Clone, Debug)]
struct ArgsSpan {
    tp: ArgsType,
    source_span: (usize, usize),
}
impl ArgsSpan {
    fn is_empty(&self) -> bool {
        match &self.tp {
            ArgsType::Empty => true,
            _ => false,
        }
    }

    fn add_source_span(mut self, other: ArgsSpan) -> ArgsSpan {
        let (start, end) = self.source_span;
        let (o_start, o_end) = other.source_span;
        self.source_span = (start.min(o_start), end.max(o_end));
        self
    }

    fn merge(source: &str, a: ArgsSpan, b: ArgsSpan) -> ArgsSpan {
        let (a_start, a_end) = a.source_span;
        let (b_start, b_end) = b.source_span;
        let new_span = (a_start.min(b_start), a_end.max(b_end));

        let new_tp = match a.tp {
            ArgsType::Empty => b.tp,
            ArgsType::Span(start, end) => match b.tp {
                ArgsType::Empty => ArgsType::Span(start, end),
                ArgsType::Span(b_start, b_end) if b_start == end => ArgsType::Span(start, b_end),
                b => ArgsType::Inline(format!("{}{}", &source[start..end], b.as_str(source))),
            }
            ArgsType::Inline(mut s) => {
                if !b.is_empty() {
                    s.push_str(b.tp.as_str(source));
                }
                ArgsType::Inline(s)
            }
        };

        ArgsSpan {
            tp: new_tp,
            source_span: new_span,
        }
    }
}

#[derive(Clone, Debug)]
struct ParserTokenCtx<'a> {
    /// The source string.
    source: &'a str,
    /// The currently parsed arguments.
    args: Vec<ArgsSpan>,

    /// Whether we are in whitespace between arguments.
    is_new_arg: bool,
    /// Whether we are in a span, whether quoted or otherwise.
    has_span: bool,
    /// The first character of the current span.
    cur_span_start: usize,
    /// A buffer for the current argument.
    cur_arg: Option<ArgsSpan>,
}
impl <'a> ParserTokenCtx<'a> {
    fn add_span(&mut self, args: ArgsSpan) {
        match self.cur_arg.take() {
            Some(x) => self.cur_arg = Some(ArgsSpan::merge(self.source, x, args)),
            None => self.cur_arg = Some(args),
        }
    }
    fn add_source_span(&mut self, args: ArgsSpan) {
        match self.cur_arg.take() {
            Some(x) => self.cur_arg = Some(x.add_source_span(args)),
            None => self.cur_arg = Some(ArgsSpan {
                tp: ArgsType::Empty,
                source_span: args.source_span,
            }),
        }
    }
    fn end_current_span(&mut self, idx: usize) -> ArgsSpan {
        if self.has_span {
            self.has_span = false;
            ArgsSpan {
                tp: ArgsType::Span(self.cur_span_start, idx),
                source_span: (self.cur_span_start, idx),
            }
        } else {
            ArgsSpan {
                tp: ArgsType::Empty,
                source_span: (idx, idx),
            }
        }
    }

    fn push_current_span(&mut self, idx: usize) {
        let new_span = self.end_current_span(idx);
        self.add_span(new_span);
    }
    fn drop_current_span(&mut self, idx: usize) {
        let new_span = self.end_current_span(idx);
        self.add_source_span(new_span);
    }
    fn push_truncated_span(&mut self, idx: usize, cut_start: usize, cut_end: usize) {
        let init_span = (self.cur_span_start, idx);
        self.cur_span_start = idx.min(self.cur_span_start + cut_start);
        let idx = self.cur_span_start.max(idx - cut_end);
        self.push_current_span(idx);
        self.add_source_span(ArgsSpan {
            tp: ArgsType::Empty,
            source_span: init_span,
        });
    }

    fn push_char(&mut self, idx: usize) {
        self.is_new_arg = false;
        if !self.has_span {
            self.cur_span_start = idx;
            self.has_span = true;
        }
    }
    fn push_new_arg(&mut self, idx: usize) {
        if !self.is_new_arg {
            self.push_current_span(idx);
            self.is_new_arg = true;
            if let Some(span) = self.cur_arg.take() {
                self.args.push(span);
            }
        }
    }
    fn push_ignored_char(&mut self, idx: usize) {
        self.is_new_arg = false;
        self.push_current_span(idx);
    }
}

/// The parsed arguments for a given input.
///
/// Note that this only stores indicies.
pub struct Args {
    args_spans: Vec<ArgsSpan>,
}
impl Args {
    pub fn parse(options: ArgParsingOptions, source: &str) -> Args {
        let mut ctx = ParserTokenCtx {
            source,
            args: Vec::new(),
            is_new_arg: true,
            has_span: false,
            cur_span_start: 0,
            cur_arg: None,
        };

        // Whether we are creating a new escape for this character.
        let mut new_escape = false;
        // Whether the current character is escaped.
        let mut is_escaped = false;
        // Whether a recovery has been started before.
        let mut has_recovered = false;

        // Where the current quote starts.
        let mut quote_start = 0;
        // The parser context to recover to if a quote is left open.
        let mut quote_recovery_state = None;
        // Whether we are in a quoted context.
        let mut is_quoted = false;

        // Whether the current quote is a markdown quote.
        let mut is_markdown_quote = false;
        // Whether the markdown quote has been properly entered.
        let mut markdown_started = false;
        // Whether the markdown parser is currently parsing a string of backticks.
        let mut markdown_quotes = false;
        // The number of markdown quotes in the current quote.
        let mut markdown_quote_count = 0;
        // A temporary counter used to count Markdown ending quotes.
        let mut markdown_end_quote_count = 0;

        // We wrap this in a loop so we can recover from unclosed quotes.
        let mut recovery_start = 0;
        'main: loop {
            let loop_start = recovery_start;
            for (idx, ch) in source[recovery_start..source.len()].char_indices() {
                let idx = loop_start + idx;
                let parse_quotes = !is_escaped && !has_recovered;
                let is_normal_quote = is_quoted && !is_markdown_quote;
                let is_markdown = is_quoted && is_markdown_quote;
                match ch {
                    // Handle escapes.
                    '\\' if !is_escaped && !is_markdown => {
                        // we commit all existing characters then push the backspace
                        // this lets us use end_span to remove this backspace later if we need to.
                        ctx.push_current_span(idx);
                        ctx.push_char(idx);
                        new_escape = true;
                    }

                    // Handle starting plain quotes.
                    '"' if parse_quotes && !is_quoted => {
                        // set up the recovery state
                        quote_start = idx;
                        quote_recovery_state = Some(ctx.clone());
                        // set up the quote state
                        is_quoted = true;
                        is_markdown_quote = false;
                        // ends the current span
                        ctx.push_ignored_char(idx);
                    }
                    // Handle ending plain quotes.
                    '"' if parse_quotes && is_normal_quote => {
                        is_quoted = false;
                        // ends the current span
                        ctx.push_ignored_char(idx);
                    }

                    // Handle starting markdown quotes.
                    '`' if parse_quotes && options.parse_markdown && !is_quoted => {
                        // set up the recovery state
                        quote_start = idx;
                        quote_recovery_state = Some(ctx.clone());
                        // set up the quote state
                        is_quoted = true;
                        is_markdown_quote = true;
                        markdown_quote_count = 1;
                        markdown_started = false;
                        markdown_quotes = true;
                        // we commit the backtick so we can handle ending quotes right.
                        ctx.push_current_span(idx);
                        ctx.push_char(idx);
                    }
                    // Handles additional backticks once markdown is parsing.
                    '`' if is_markdown => {
                        if markdown_quotes {
                            if markdown_started { markdown_end_quote_count += 1; }
                            else { markdown_quote_count += 1; }
                        } else {
                            markdown_quotes = true;
                            markdown_end_quote_count = 1;
                        }
                    }
                    // Handle the contents of markdown quotes
                    _ if is_markdown => {
                        if markdown_quotes {
                            if !markdown_started {
                                // end of the starting backtick chain
                                markdown_started = true;
                            } else if markdown_end_quote_count <= markdown_quote_count {
                                // end of a normal backtick chain
                                is_quoted = false;
                                ctx.push_truncated_span(
                                    idx, markdown_end_quote_count, markdown_end_quote_count,
                                );

                                // Reparse this using the normal parser.
                                recovery_start = idx;
                                continue 'main;
                            } else {
                                // we ignore these
                            }
                            markdown_quotes = false;
                        }

                        ctx.push_char(idx);
                    }

                    // Handle whitespace.
                    _ if ch.is_ascii_whitespace() && !is_quoted && !is_escaped =>
                        ctx.push_new_arg(idx),

                    // Handle an escaped \ or ".
                    '\\' | '"' if is_escaped => {
                        ctx.drop_current_span(idx); // remove the backslash
                        ctx.push_char(idx); // add the character
                    }
                    // Handle an escaped whitespace character.
                    _ if ch.is_ascii_whitespace() && is_escaped && !is_quoted => {
                        ctx.drop_current_span(idx); // remove the backslash
                        ctx.push_char(idx); // add the character
                    }
                    // End the argument if this is whitespace.
                    _ => ctx.push_char(idx),
                }

                // Adjust the escape state.
                is_escaped = new_escape;
                new_escape = false;
            }

            // Check for the end of a markdown quote chain
            if is_quoted && is_markdown_quote && // we are in a markdown quote
                markdown_quotes && markdown_started && // we are in an ending quote chain
                markdown_end_quote_count <= markdown_quote_count
            {
                ctx.push_truncated_span(
                    source.len(), markdown_end_quote_count, markdown_end_quote_count,
                );
                ctx.push_new_arg(source.len());
                break 'main;
            }

            // Recover from an unterminated quote.
            if is_quoted {
                // reset quote state
                new_escape = false;
                is_escaped = false;
                has_recovered = true;
                is_quoted = false;

                // set up recovery state
                recovery_start = quote_start;
                ctx = quote_recovery_state.take().expect("No quote recovery state?");
                continue 'main;
            }

            // Finish parsing
            ctx.push_new_arg(source.len());
            break 'main;
        }

        Args {
            args_spans: ctx.args,
        }
    }

    pub fn len(&self) -> usize {
        self.args_spans.len()
    }

    pub fn arg<'a: 'c, 'b: 'c, 'c>(&'a self, source: &'b str, i: usize) -> &'c str {
        self.args_spans[i].tp.as_str(source)
    }
    pub fn source_span(&self, i: usize) -> (usize, usize) {
        self.args_spans[i].source_span
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_parser(options: ArgParsingOptions, source: &str, expected: &[&str]) {
        let parsed = Args::parse(options, source);

        let mut args = Vec::new();
        for i in 0..parsed.len() {
            args.push(parsed.arg(source, i));
        }
        assert_eq!(args, expected);
    }
    fn check_parser_full(options: ArgParsingOptions, source: &str, expected: &[&str]) {
        let parsed = Args::parse(options, source);

        let mut args = Vec::new();
        for i in 0..parsed.len() {
            let (start, end) = parsed.source_span(i);
            args.push(&source[start..end]);
        }
        assert_eq!(args, expected);
    }

    #[test]
    fn basic_test() {
        let options = ArgParsingOptions::default();
        check_parser(options, "   a b   c   ", &["a", "b", "c"]);
        check_parser(options, "a b c", &["a", "b", "c"]);
        check_parser(options, "a b   c   ", &["a", "b", "c"]);
        check_parser(options, "    a    b c", &["a", "b", "c"]);
        check_parser(options, "   aaaaa bbbbb   ccccc   ", &["aaaaa", "bbbbb", "ccccc"]);
        check_parser(options, "", &[]);
        check_parser(options, "           ", &[]);
    }

    #[test]
    fn escaped_test() {
        let options = ArgParsingOptions::default();
        check_parser(options, r"\a\b\c", &[r"\a\b\c"]);
        check_parser(options, r"\a\ \c", &[r"\a \c"]);
        check_parser(options, r#""abc\ def""#, &[r"abc\ def"]);
        check_parser(options, r#""abc\"def""#, &[r#"abc"def"#]);
    }

    #[test]
    fn quoted_test() {
        let options = ArgParsingOptions::default();
        check_parser(options, r#""abc def" def"#, &["abc def", "def"]);
        check_parser(options, r#"a" b "c"#, &["a b c"]);
        check_parser(options, r#"a"b"#, &["a\"b"]);
        check_parser(options, r#"a" b"#, &["a\"", "b"]);
        check_parser(options, r#"a\"b""#, &["a\"b\""]);
    }

    #[test]
    fn disable_markdown_test() {
        let options = ArgParsingOptions::default();
        check_parser(options, "   ```a b   c```   ", &["```a", "b", "c```"]);
        check_parser(options, "a ``b   c``   ", &["a", "``b", "c``"]);
    }

    #[test]
    fn markdown_test() {
        let options = ArgParsingOptions::default().parse_markdown();
        check_parser(options, "   ```a b   c```   ", &["a b   c"]);
        check_parser(options, "a ``b   c``   ", &["a", "b   c"]);
        check_parser(options, "    a    `b c`", &["a", "b c"]);
        check_parser(options, "``abc`", &["`abc"]);
        check_parser(options, "``abc```abc`", &["`abc```abc"]);
        check_parser(options, "``abc", &["``abc"]);
        check_parser(options, "abc``abc```abc", &["abc``abc```abc"]);
    }

    #[test]
    fn mixed_test() {
        let options = ArgParsingOptions::default().parse_markdown();
        check_parser(options, r#"`abc "def` ghi""# , &[r#"abc "def"#, r#"ghi""#]);
        check_parser(options, r#""abc `def" ghi`"# , &[r"abc `def", r"ghi`"]);
    }

    #[test]
    fn source_span() {
        let options = ArgParsingOptions::default().parse_markdown();
        check_parser_full(options, r#"`abc "def` ghi""# , &[r#"`abc "def`"#, r#"ghi""#]);
        check_parser_full(options, r#""abc `def" ghi`"# , &[r#""abc `def""#, r"ghi`"]);
    }
}
