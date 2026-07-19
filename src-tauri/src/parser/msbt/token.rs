#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPart {
    Text(String),
    Start {
        group: u16,
        kind: u16,
        args: Vec<u8>,
    },
    End {
        group: u16,
        kind: u16,
    },
}
