//! Lexical analysis: turns SQL text into a list of positioned tokens.

#[cfg(test)]
mod tests;

use wasm_dbms_api::prelude::SqlError;

/// 1-based source position of a token.
///
/// Columns count characters, not bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Position {
    pub(crate) line: usize,
    pub(crate) col: usize,
}

impl Position {
    /// Builds a [`SqlError::Parse`] located at this position.
    pub(crate) fn error(self, msg: impl Into<String>) -> SqlError {
        SqlError::Parse {
            line: self.line,
            col: self.col,
            msg: msg.into(),
        }
    }
}

macro_rules! keywords {
    ($($variant:ident => $text:literal),+ $(,)?) => {
        /// Reserved words of the SQL dialect.
        ///
        /// A reserved word can only be used as a table or column name when it
        /// is written between double quotes.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Keyword {
            $($variant),+
        }

        impl Keyword {
            /// Every reserved word, in declaration order.
            pub(crate) const ALL: &'static [Keyword] = &[$(Keyword::$variant),+];

            /// Returns the canonical upper-case spelling of the keyword.
            pub(crate) fn as_str(self) -> &'static str {
                match self {
                    $(Keyword::$variant => $text),+
                }
            }

            /// Returns the keyword spelled `word`, ignoring ASCII case.
            pub(crate) fn from_word(word: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|keyword| keyword.as_str().eq_ignore_ascii_case(word))
            }
        }
    };
}

keywords! {
    And => "AND",
    As => "AS",
    Asc => "ASC",
    Begin => "BEGIN",
    By => "BY",
    Cascade => "CASCADE",
    Commit => "COMMIT",
    Cross => "CROSS",
    Delete => "DELETE",
    Desc => "DESC",
    Distinct => "DISTINCT",
    Except => "EXCEPT",
    False => "FALSE",
    From => "FROM",
    Full => "FULL",
    Group => "GROUP",
    Having => "HAVING",
    In => "IN",
    Inner => "INNER",
    Insert => "INSERT",
    Intersect => "INTERSECT",
    Into => "INTO",
    Is => "IS",
    Join => "JOIN",
    Left => "LEFT",
    Like => "LIKE",
    Limit => "LIMIT",
    Natural => "NATURAL",
    Not => "NOT",
    Null => "NULL",
    Offset => "OFFSET",
    On => "ON",
    Or => "OR",
    Order => "ORDER",
    Outer => "OUTER",
    Restrict => "RESTRICT",
    Right => "RIGHT",
    Rollback => "ROLLBACK",
    Select => "SELECT",
    Set => "SET",
    Transaction => "TRANSACTION",
    True => "TRUE",
    Union => "UNION",
    Update => "UPDATE",
    Using => "USING",
    Values => "VALUES",
    Where => "WHERE",
}

/// A lexical unit of a SQL statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    /// A reserved word.
    Keyword(Keyword),
    /// A table, column, alias, or function name; quoted or not.
    Identifier(String),
    /// An unsigned integer literal.
    Integer(u64),
    /// An unsigned decimal literal, kept as written (`digits.digits`).
    Float(String),
    /// The content of a single-quoted string literal, with `''` unescaped.
    String(String),
    /// The `?` parameter placeholder.
    Placeholder,
    Comma,
    Dot,
    LeftParen,
    RightParen,
    Star,
    Semicolon,
    Minus,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    /// End of the statement text.
    Eof,
}

impl Token {
    /// Describes the token for the `found ...` part of an error message.
    pub(crate) fn describe(&self) -> String {
        match self {
            Token::Keyword(keyword) => format!("keyword `{keyword}`", keyword = keyword.as_str()),
            Token::Identifier(name) => format!("identifier `{name}`"),
            Token::Integer(value) => format!("number `{value}`"),
            Token::Float(value) => format!("number `{value}`"),
            Token::String(_) => "string literal".to_string(),
            Token::Placeholder => "`?`".to_string(),
            Token::Comma => "`,`".to_string(),
            Token::Dot => "`.`".to_string(),
            Token::LeftParen => "`(`".to_string(),
            Token::RightParen => "`)`".to_string(),
            Token::Star => "`*`".to_string(),
            Token::Semicolon => "`;`".to_string(),
            Token::Minus => "`-`".to_string(),
            Token::Eq => "`=`".to_string(),
            Token::NotEq => "`!=`".to_string(),
            Token::Lt => "`<`".to_string(),
            Token::LtEq => "`<=`".to_string(),
            Token::Gt => "`>`".to_string(),
            Token::GtEq => "`>=`".to_string(),
            Token::Eof => "end of input".to_string(),
        }
    }
}

/// A [`Token`] together with the position of its first character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spanned {
    pub(crate) token: Token,
    pub(crate) position: Position,
}

/// Splits `sql` into tokens.
///
/// Whitespace and comments (`-- line` and `/* block */`) are dropped. The
/// returned list always ends with a [`Token::Eof`] positioned right after the
/// last character.
///
/// # Errors
///
/// Returns [`SqlError::Parse`] for an unterminated string, quoted identifier,
/// or block comment, an empty quoted identifier, a malformed or oversized
/// number, a backtick, a named or numbered placeholder, or any character that
/// is not part of the dialect.
pub(crate) fn tokenize(sql: &str) -> Result<Vec<Spanned>, SqlError> {
    let mut lexer = Lexer::new(sql);
    let mut tokens = Vec::new();
    loop {
        lexer.skip_trivia()?;
        let position = lexer.position;
        let token = lexer.next_token()?;
        let done = token == Token::Eof;
        tokens.push(Spanned { token, position });
        if done {
            return Ok(tokens);
        }
    }
}

/// Character cursor over the statement text.
struct Lexer {
    chars: Vec<char>,
    index: usize,
    position: Position,
}

impl Lexer {
    fn new(sql: &str) -> Self {
        Self {
            chars: sql.chars().collect(),
            index: 0,
            position: Position { line: 1, col: 1 },
        }
    }

    /// Returns the character `offset` places after the cursor.
    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).copied()
    }

    fn peek(&self) -> Option<char> {
        self.peek_at(0)
    }

    /// Consumes one character, keeping the line and column up to date.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.index += 1;
        if c == '\n' {
            self.position.line += 1;
            self.position.col = 1;
        } else {
            self.position.col += 1;
        }
        Some(c)
    }

    /// Consumes one character and returns `token`.
    fn single(&mut self, token: Token) -> Token {
        self.bump();
        token
    }

    /// Consumes two characters and returns `token`.
    fn double(&mut self, token: Token) -> Token {
        self.bump();
        self.bump();
        token
    }

    /// Skips whitespace, `-- line` comments, and `/* block */` comments.
    fn skip_trivia(&mut self) -> Result<(), SqlError> {
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some(c), _) if c.is_whitespace() => {
                    self.bump();
                }
                (Some('-'), Some('-')) => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                (Some('/'), Some('*')) => self.skip_block_comment()?,
                _ => return Ok(()),
            }
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), SqlError> {
        let start = self.position;
        self.bump();
        self.bump();
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some('*'), Some('/')) => {
                    self.bump();
                    self.bump();
                    return Ok(());
                }
                (Some(_), _) => {
                    self.bump();
                }
                (None, _) => return Err(start.error("unterminated block comment")),
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, SqlError> {
        let position = self.position;
        let Some(c) = self.peek() else {
            return Ok(Token::Eof);
        };
        let token = match (c, self.peek_at(1)) {
            (',', _) => self.single(Token::Comma),
            ('.', _) => self.single(Token::Dot),
            ('(', _) => self.single(Token::LeftParen),
            (')', _) => self.single(Token::RightParen),
            ('*', _) => self.single(Token::Star),
            (';', _) => self.single(Token::Semicolon),
            ('-', _) => self.single(Token::Minus),
            ('?', _) => self.single(Token::Placeholder),
            ('=', _) => self.single(Token::Eq),
            ('!', Some('=')) | ('<', Some('>')) => self.double(Token::NotEq),
            ('<', Some('=')) => self.double(Token::LtEq),
            ('<', _) => self.single(Token::Lt),
            ('>', Some('=')) => self.double(Token::GtEq),
            ('>', _) => self.single(Token::Gt),
            ('\'', _) => Token::String(self.quoted('\'', "unterminated string literal")?),
            ('"', _) => {
                let name = self.quoted('"', "unterminated quoted identifier")?;
                if name.is_empty() {
                    return Err(position.error("quoted identifier cannot be empty"));
                }
                Token::Identifier(name)
            }
            ('`', _) => {
                return Err(position
                    .error("backtick-quoted identifiers are not supported; use double quotes"));
            }
            (':' | '$' | '@', _) => {
                return Err(
                    position.error("named and numbered placeholders are not supported; use `?`")
                );
            }
            (c, _) if c.is_ascii_digit() => self.number()?,
            (c, _) if c.is_ascii_alphabetic() || c == '_' => self.word(),
            (c, _) => return Err(position.error(format!("unexpected character `{c}`"))),
        };
        Ok(token)
    }

    /// Reads a `quote`-delimited token, where a doubled quote stands for one quote.
    fn quoted(&mut self, quote: char, unterminated: &str) -> Result<String, SqlError> {
        let start = self.position;
        self.bump();
        let mut content = String::new();
        loop {
            match (self.bump(), self.peek()) {
                (Some(c), Some(next)) if c == quote && next == quote => {
                    self.bump();
                    content.push(quote);
                }
                (Some(c), _) if c == quote => return Ok(content),
                (Some(c), _) => content.push(c),
                (None, _) => return Err(start.error(unterminated)),
            }
        }
    }

    /// Reads an integer (`digits`) or decimal (`digits.digits`) literal.
    fn number(&mut self) -> Result<Token, SqlError> {
        let start = self.position;
        let mut text = self.digits();
        let is_float =
            self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit());
        if is_float {
            self.bump();
            text.push('.');
            text.push_str(&self.digits());
        }
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return Err(start.error("invalid numeric literal"));
        }
        if is_float {
            return Ok(Token::Float(text));
        }
        text.parse()
            .map(Token::Integer)
            .map_err(|_| start.error("integer literal is too large"))
    }

    fn digits(&mut self) -> String {
        let mut digits = String::new();
        while let Some(c) = self.peek().filter(char::is_ascii_digit) {
            self.bump();
            digits.push(c);
        }
        digits
    }

    /// Reads a keyword or an unquoted identifier.
    fn word(&mut self) -> Token {
        let mut word = String::new();
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            self.bump();
            word.push(c);
        }
        match Keyword::from_word(&word) {
            Some(keyword) => Token::Keyword(keyword),
            None => Token::Identifier(word),
        }
    }
}
