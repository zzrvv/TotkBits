use crate::{
    file_format::BinTextFile::OpenedFile,
    parser::{binary::Endian as BinaryEndian, msbt::Msbt},
    InternalFile::InternalFile,
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{is_msyt, TotkFileType, ZstdDictionary},
};
use std::path::Path;

pub struct MsbtFile {
    pub path: Pathlib,
    pub endian: roead::Endian,
    pub file_type: TotkFileType,
    pub text: String,
}
impl MsbtFile {
    fn parse(data: &[u8]) -> Option<(Msbt, String, roead::Endian)> {
        if !is_msyt(data) {
            return None;
        }
        let msbt = Msbt::from_bytes(data).ok()?;
        let endian = match msbt.header.endian {
            BinaryEndian::Little => roead::Endian::Little,
            BinaryEndian::Big => roead::Endian::Big,
        };
        let text = crate::parser::msbt::editable::serialize(&msbt);
        Some((msbt, text, endian))
    }

    pub fn from_binary(data: Vec<u8>, path: Option<String>) -> Option<Self> {
        let file = Msbt::from_bytes(&data).ok()?;
        let endian = match file.header.endian {
            crate::parser::binary::Endian::Little => roead::Endian::Little,
            crate::parser::binary::Endian::Big => roead::Endian::Big,
        };
        Some(Self {
            path: Pathlib::new(path.unwrap_or_default()),
            endian,
            file_type: TotkFileType::Msbt,
            text: crate::parser::msbt::editable::serialize(&file),
        })
    }
    pub fn from_filepath(path: &str) -> Option<Self> {
        Self::from_binary(std::fs::read(path).ok()?, Some(path.into()))
    }
    pub fn open_mstb<P: AsRef<Path>>(path: P) -> Option<(OpenedFile<'static>, SendData)> {
        let name = path.as_ref().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).ok()?;
        let (parsed, text, endian) = Self::parse(&bytes)?;
        let mut opened = OpenedFile::default();
        opened.path = Pathlib::new(&name);
        opened.endian = Some(endian);
        opened.file_type = TotkFileType::Msbt;
        opened.msyt = Some(parsed);
        let mut sent = SendData::default();
        sent.path = Pathlib::new(&name);
        sent.text = text;
        sent.status_text = format!("Opened {name}");
        sent.get_file_label(TotkFileType::Msbt, Some(endian));
        Some((opened, sent))
    }

    pub fn open_internal(
        path: &str,
        data: &[u8],
        compression: Option<ZstdDictionary>,
    ) -> Option<(InternalFile<'static>, String)> {
        let (parsed, text, endian) = Self::parse(data)?;
        let mut internal = InternalFile::default();
        internal.path = Pathlib::new(path);
        internal.endian = Some(endian);
        internal.file_type = TotkFileType::Msbt;
        internal.msyt = Some(parsed);
        internal.compression = compression;
        Some((internal, text))
    }

    pub fn text_to_binary(
        text: &str,
        path: &str,
        parsed: Option<&Msbt>,
        is_internal: bool,
    ) -> Option<Vec<u8>> {
        if is_internal && parsed.is_none() {
            Self::show_missing_internal_handle();
            return None;
        }
        let disk_template;
        let template = match parsed {
            Some(parsed) => parsed,
            None => {
                disk_template = Msbt::from_bytes(&std::fs::read(path).ok()?).ok()?;
                &disk_template
            }
        };
        crate::parser::msbt::editable::deserialize(template, text)
            .ok()?
            .to_bytes()
            .ok()
    }

    fn show_missing_internal_handle() {
        rfd::MessageDialog::new()
            .set_title("TotkBits - MSBT save error")
            .set_description("Cannot save the internal MSBT file because its parsed archive handle is missing. Close and reopen the internal file, then try again.")
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}
pub fn str_endian_to_roead(endian: &str) -> roead::Endian {
    if endian == "BE" {
        roead::Endian::Big
    } else {
        roead::Endian::Little
    }
}

#[cfg(test)]
mod tests {
    use super::MsbtFile;
    use crate::Zstd::TotkFileType;
    use std::path::PathBuf;

    fn sample() -> Option<(PathBuf, Vec<u8>)> {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/EUen/ActorMsg/Attachment.msbt");
        std::fs::read(&path).ok().map(|data| (path, data))
    }

    #[test]
    fn opens_internal_msbt_bytes() {
        let Some((_path, data)) = sample() else {
            return;
        };
        let (opened, text) = MsbtFile::open_internal("ActorMsg/Attachment.msbt", &data, None)
            .expect("open internal MSBT");
        assert_eq!(opened.file_type, TotkFileType::Msbt);
        assert!(text.starts_with("%%%\r\n"));
        assert!(opened.msyt.is_some());

        let rebuilt = MsbtFile::text_to_binary(
            &text,
            "ActorMsg/Attachment.msbt",
            opened.msyt.as_ref(),
            true,
        )
        .expect("save internal MSBT using retained parser state");
        assert_eq!(rebuilt, data);
    }

    #[test]
    fn opens_regular_msbt_file() {
        let Some((path, _data)) = sample() else {
            return;
        };
        let (opened, sent) = MsbtFile::open_mstb(&path).expect("open disk MSBT");
        assert_eq!(opened.file_type, TotkFileType::Msbt);
        assert!(opened.msyt.is_some());
        assert!(sent.text.starts_with("%%%\r\n"));
    }
}
