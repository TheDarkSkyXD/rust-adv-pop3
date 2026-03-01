/// Mailbox statistics returned by the `STAT` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// Number of messages in the mailbox.
    pub message_count: u32,
    /// Total size of the mailbox in bytes.
    pub mailbox_size: u64,
}

/// A single entry from the `LIST` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// The message number.
    pub message_id: u32,
    /// Size of the message in bytes.
    pub size: u64,
}

/// A single entry from the `UIDL` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UidlEntry {
    /// The message number.
    pub message_id: u32,
    /// The unique identifier for the message.
    pub unique_id: String,
}

/// A retrieved message (from `RETR` or `TOP`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The full message content, dot-unstuffed per RFC 1939.
    pub data: String,
}

/// A server capability from the `CAPA` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// The capability name (e.g. "TOP", "UIDL", "SASL").
    pub name: String,
    /// Optional arguments for the capability.
    pub arguments: Vec<String>,
}
