//! Domain: the `pmcp-mail v1` text envelope carried by a mailbox comment.

pub const MAGIC: &str = "pmcp-mail v1";

/// A message as written by its sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub re: Option<String>,
    pub body: String,
}

/// A message as stored in a mailbox, with its receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: u64,
    pub created_at: String,
    pub envelope: Envelope,
    pub read: bool,
    pub acked: bool,
}

/// Actor names are lower-case words joined by hyphens, like `oss-desktop-2`.
pub fn valid_actor(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

impl Envelope {
    pub fn render(&self) -> String {
        let mut s = format!(
            "{MAGIC}\nfrom: {}\nto: {}\nsubject: {}\n",
            self.from, self.to, self.subject
        );
        if let Some(re) = &self.re {
            s.push_str(&format!("re: {re}\n"));
        }
        s.push_str("---\n");
        s.push_str(self.body.trim_end());
        s.push('\n');
        s
    }

    /// Parses a comment body; `None` when it is not a mailbox message.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.replace("\r\n", "\n");
        let mut lines = text.lines();
        if lines.next()?.trim() != MAGIC {
            return None;
        }
        let (mut from, mut to, mut subject, mut re) = (None, None, String::new(), None);
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();
            match key.trim() {
                "from" => from = Some(value),
                "to" => to = Some(value),
                "subject" => subject = value,
                "re" if !value.is_empty() => re = Some(value),
                _ => {}
            }
        }
        let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        Some(Self {
            from: from.filter(|f| valid_actor(f))?,
            to: to.filter(|t| valid_actor(t))?,
            subject,
            re,
            body,
        })
    }
}
