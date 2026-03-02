/// The current state of the POP3 session.
///
/// Used by [`Pop3Client::state()`](crate::Pop3Client::state) to report
/// the current session phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// AUTHORIZATION state — connected to the server but not yet authenticated.
    Connected,
    /// TRANSACTION state — successfully authenticated; mailbox commands are available.
    Authenticated,
    /// Session has ended (after QUIT).
    Disconnected,
}

/// Mailbox statistics returned by the `STAT` command.
///
/// Returned by [`Pop3Client::stat()`](crate::Pop3Client::stat).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// Number of messages currently in the mailbox.
    pub message_count: u32,
    /// Total size of all messages in the mailbox, in bytes.
    pub mailbox_size: u64,
}

/// A single entry from the `LIST` command, identifying a message and its size.
///
/// Returned by [`Pop3Client::list()`](crate::Pop3Client::list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// The message number (1-based per RFC 1939).
    pub message_id: u32,
    /// Size of the message in bytes.
    pub size: u64,
}

/// A single entry from the `UIDL` command, pairing a message number with its unique ID.
///
/// Returned by [`Pop3Client::uidl()`](crate::Pop3Client::uidl).
/// Unique IDs are stable across sessions and can be used to detect previously
/// downloaded messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UidlEntry {
    /// The message number (1-based, session-local).
    pub message_id: u32,
    /// The server-assigned unique identifier for this message (stable across sessions).
    pub unique_id: String,
}

/// A retrieved message returned by `RETR` or `TOP`.
///
/// Returned by [`Pop3Client::retr()`](crate::Pop3Client::retr) and
/// [`Pop3Client::top()`](crate::Pop3Client::top).
///
/// The `data` field contains raw RFC 2822 message content with dot-unstuffing
/// applied per RFC 1939 (leading `..` on a line is reduced to `.`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The full message content (headers + body), dot-unstuffed per RFC 1939.
    pub data: String,
}

/// A server capability from the `CAPA` command (RFC 2449).
///
/// Returned by [`Pop3Client::capa()`](crate::Pop3Client::capa).
/// Common capabilities: `TOP`, `UIDL`, `SASL`, `STLS`, `RESP-CODES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// The capability keyword (e.g., `"TOP"`, `"UIDL"`, `"SASL"`).
    pub name: String,
    /// Optional capability arguments (e.g., `["PLAIN", "GSSAPI"]` for `SASL`).
    pub arguments: Vec<String>,
}

/// A parsed RFC 5322 / MIME email message.
///
/// This is a type alias for [`mail_parser::Message<'static>`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html)
/// from `mail-parser` 0.11.x.  The message owns all its data (no borrowed
/// references), so it can be stored, moved, and sent across threads freely.
///
/// Common accessors:
///
/// | Method | Returns | Description |
/// |--------|---------|-------------|
/// | [`subject()`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html#method.subject) | `Option<&str>` | The `Subject` header |
/// | [`from()`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html#method.from) | `Option<&Address>` | The `From` header |
/// | [`body_text(0)`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html#method.body_text) | `Option<Cow<str>>` | First text/plain body part |
/// | [`body_html(0)`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html#method.body_html) | `Option<Cow<str>>` | First text/html body part |
/// | [`attachment_count()`](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html#method.attachment_count) | `usize` | Number of attachments |
///
/// See the [mail-parser documentation](https://docs.rs/mail-parser/0.11/mail_parser/struct.Message.html)
/// for the full API.
///
/// Requires the `mime` feature flag.
#[cfg(feature = "mime")]
pub type ParsedMessage = mail_parser::Message<'static>;
