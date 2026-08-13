use std::io::{self, ErrorKind};

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeType {
    FloatSelector = 1,
    StringSelector,
    SkeletalAnimation,
    State,
    Unknown2,
    OneDimensionalBlender,
    Sequential,
    IntSelector,
    Simultaneous,
    Event,
    MaterialAnimation,
    FrameController,
    DummyAnimation,
    RandomSelector,
    Unknown4,
    PreviousTagSelector,
    BonePositionSelector,
    BoneAnimation,
    InitialFrame,
    BoneBlender,
    BoolSelector,
    Alert,
    SubtractAnimation,
    ShapeAnimation,
    Unknown7,
}

impl NodeType {
    pub fn from_name(name: &str) -> io::Result<Self> {
        (1..=25)
            .map(Self::from_u16)
            .find_map(|value| value.ok().filter(|kind| kind.name() == name))
            .ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("invalid ASB node type {name}"),
                )
            })
    }
    pub fn as_u16(self) -> u16 {
        self as u16
    }
    pub fn from_u16(value: u16) -> io::Result<Self> {
        match value {
            1 => Ok(Self::FloatSelector),
            2 => Ok(Self::StringSelector),
            3 => Ok(Self::SkeletalAnimation),
            4 => Ok(Self::State),
            5 => Ok(Self::Unknown2),
            6 => Ok(Self::OneDimensionalBlender),
            7 => Ok(Self::Sequential),
            8 => Ok(Self::IntSelector),
            9 => Ok(Self::Simultaneous),
            10 => Ok(Self::Event),
            11 => Ok(Self::MaterialAnimation),
            12 => Ok(Self::FrameController),
            13 => Ok(Self::DummyAnimation),
            14 => Ok(Self::RandomSelector),
            15 => Ok(Self::Unknown4),
            16 => Ok(Self::PreviousTagSelector),
            17 => Ok(Self::BonePositionSelector),
            18 => Ok(Self::BoneAnimation),
            19 => Ok(Self::InitialFrame),
            20 => Ok(Self::BoneBlender),
            21 => Ok(Self::BoolSelector),
            22 => Ok(Self::Alert),
            23 => Ok(Self::SubtractAnimation),
            24 => Ok(Self::ShapeAnimation),
            25 => Ok(Self::Unknown7),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("invalid ASB node type {value}"),
            )),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::FloatSelector => "FloatSelector",
            Self::StringSelector => "StringSelector",
            Self::SkeletalAnimation => "SkeletalAnimation",
            Self::State => "State",
            Self::Unknown2 => "Unknown2",
            Self::OneDimensionalBlender => "OneDimensionalBlender",
            Self::Sequential => "Sequential",
            Self::IntSelector => "IntSelector",
            Self::Simultaneous => "Simultaneous",
            Self::Event => "Event",
            Self::MaterialAnimation => "MaterialAnimation",
            Self::FrameController => "FrameController",
            Self::DummyAnimation => "DummyAnimation",
            Self::RandomSelector => "RandomSelector",
            Self::Unknown4 => "Unknown4",
            Self::PreviousTagSelector => "PreviousTagSelector",
            Self::BonePositionSelector => "BonePositionSelector",
            Self::BoneAnimation => "BoneAnimation",
            Self::InitialFrame => "InitialFrame",
            Self::BoneBlender => "BoneBlender",
            Self::BoolSelector => "BoolSelector",
            Self::Alert => "Alert",
            Self::SubtractAnimation => "SubtractAnimation",
            Self::ShapeAnimation => "ShapeAnimation",
            Self::Unknown7 => "Unknown7",
        }
    }
}
