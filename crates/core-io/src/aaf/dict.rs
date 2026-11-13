//! The AAF IDs we use (plan 6f): the class AUIDs and property IDs of the
//! classes Rulings 1, 4 and 11 name, the definition AUIDs of the plan's
//! table, and the decoders of the values we read.
//!
//! Class AUIDs and PIDs are typed from the AAF Object Specification v1.1's
//! class tables, cross-checked against the model tables at
//! <https://github.com/markreidvfx/pyaaf2> (MIT, at the commit the plan
//! pins); definition AUIDs are the plan's table.

use super::store::{Auid, MobId, Prop};
use crate::limits::{AafLimits, Limits};

/// Class AUIDs, one per class.
pub mod class {
    use super::Auid;

    /// `Root`, the class of the root storage (Low-Level Container
    /// Specification).
    pub const ROOT: Auid = Auid::from_fields(
        0xb3b398a5,
        0x1c90,
        0x11d4,
        [0x80, 0x53, 0x08, 0x00, 0x36, 0x21, 0x08, 0x04],
    );
    /// `InterchangeObject` (Object Specification, the InterchangeObject class table).
    pub const INTERCHANGE_OBJECT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0100,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Component` (Object Specification, the Component class table).
    pub const COMPONENT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0200,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Segment` (Object Specification, the Segment class table).
    pub const SEGMENT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0300,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Event` (Object Specification, the Event class table).
    pub const EVENT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0600,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `CommentMarker` (Object Specification, the CommentMarker class table).
    pub const COMMENT_MARKER: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0800,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Filler` (Object Specification, the Filler class table).
    pub const FILLER: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0900,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `OperationGroup` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0a00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Sequence` (Object Specification, the Sequence class table).
    pub const SEQUENCE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x0f00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `SourceReference` (Object Specification, the SourceReference class table).
    pub const SOURCE_REFERENCE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `SourceClip` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1100,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Timecode` (Object Specification, the Timecode class table).
    pub const TIMECODE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1400,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Transition` (Object Specification, the Transition class table).
    pub const TRANSITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1700,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ContentStorage` (Object Specification, the ContentStorage class table).
    pub const CONTENT_STORAGE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1800,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ControlPoint` (Object Specification, the ControlPoint class table).
    pub const CONTROL_POINT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1900,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `DefinitionObject` (Object Specification, the DefinitionObject class table).
    pub const DEFINITION_OBJECT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1a00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `DataDefinition` (Object Specification, the DataDefinition class table).
    pub const DATA_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1b00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `OperationDefinition` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1c00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ParameterDefinition` (Object Specification, the ParameterDefinition class table).
    pub const PARAMETER_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1d00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `CodecDefinition` (Object Specification, the CodecDefinition class table).
    pub const CODEC_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x1f00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ContainerDefinition` (Object Specification, the ContainerDefinition class table).
    pub const CONTAINER_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `InterpolationDefinition` (Object Specification, the InterpolationDefinition class table).
    pub const INTERPOLATION_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2100,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Dictionary` (Object Specification, the Dictionary class table).
    pub const DICTIONARY: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2200,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `EssenceData` (Object Specification, the EssenceData class table).
    pub const ESSENCE_DATA: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2300,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `EssenceDescriptor` (Object Specification, the EssenceDescriptor class table).
    pub const ESSENCE_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2400,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `FileDescriptor` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2500,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `AIFCDescriptor` (Object Specification, the AIFCDescriptor class table).
    pub const AIFC_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2600,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `WAVEDescriptor` (Object Specification, the WAVEDescriptor class table).
    pub const WAVE_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2c00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Header` (Object Specification, the Header class table).
    pub const HEADER: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x2f00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Identification` (Object Specification, the Identification class table).
    pub const IDENTIFICATION: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Locator` (Object Specification, the Locator class table).
    pub const LOCATOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3100,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `NetworkLocator` (Object Specification, the NetworkLocator class table).
    pub const NETWORK_LOCATOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3200,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Mob` (Object Specification, the Mob class table).
    pub const MOB: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3400,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `CompositionMob` (Object Specification, the CompositionMob class table).
    pub const COMPOSITION_MOB: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3500,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `MasterMob` (Object Specification, the MasterMob class table).
    pub const MASTER_MOB: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3600,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `SourceMob` (Object Specification, the SourceMob class table).
    pub const SOURCE_MOB: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3700,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `MobSlot` (Object Specification, the MobSlot class table).
    pub const MOB_SLOT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3800,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `EventMobSlot` (Object Specification, the EventMobSlot class table).
    pub const EVENT_MOB_SLOT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3900,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `TimelineMobSlot` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3b00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `Parameter` (Object Specification, the Parameter class table).
    pub const PARAMETER: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3c00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ConstantValue` (Object Specification, the ConstantValue class table).
    pub const CONSTANT_VALUE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3d00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `VaryingValue` (Object Specification, the VaryingValue class table).
    pub const VARYING_VALUE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3e00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `TaggedValue` (Object Specification, the TaggedValue class table).
    pub const TAGGED_VALUE: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x3f00,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `DescriptiveMarker` (Object Specification, the DescriptiveMarker class table).
    pub const DESCRIPTIVE_MARKER: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x4100,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `SoundDescriptor` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x4200,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `PCMDescriptor` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR: Auid = Auid::from_fields(
        0x0d010101,
        0x0101,
        0x4800,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `ClassDefinition` (Object Specification, the ClassDefinition class table).
    pub const CLASS_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0201,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `PropertyDefinition` (Object Specification, the PropertyDefinition class table).
    pub const PROPERTY_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0202,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `TypeDefinition` (Object Specification, the TypeDefinition class table).
    pub const TYPE_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0203,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `MetaDefinition` (Object Specification, the MetaDefinition class table).
    pub const META_DEFINITION: Auid = Auid::from_fields(
        0x0d010101,
        0x0224,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
    /// `MetaDictionary` (Object Specification, the MetaDictionary class table).
    pub const META_DICTIONARY: Auid = Auid::from_fields(
        0x0d010101,
        0x0225,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01],
    );
}

/// Property IDs, named `<CLASS>_<PROPERTY>`.
pub mod pid {
    /// `Root::MetaDictionary` (Low-Level Container Specification).
    pub const ROOT_META_DICTIONARY: u16 = 0x0001;
    /// `Root::Header` (Low-Level Container Specification).
    pub const ROOT_HEADER: u16 = 0x0002;
    /// `InterchangeObject::ObjClass` (Object Specification, the InterchangeObject class table).
    pub const INTERCHANGE_OBJECT_OBJ_CLASS: u16 = 0x0101;
    /// `InterchangeObject::Generation` (Object Specification, the InterchangeObject class table).
    pub const INTERCHANGE_OBJECT_GENERATION: u16 = 0x0102;
    /// `Component::DataDefinition` (Object Specification, the Component class table).
    pub const COMPONENT_DATA_DEFINITION: u16 = 0x0201;
    /// `Component::Length` (Object Specification, the Component class table).
    pub const COMPONENT_LENGTH: u16 = 0x0202;
    /// `Component::KLVData` (Object Specification, the Component class table).
    pub const COMPONENT_KLV_DATA: u16 = 0x0203;
    /// `Component::UserComments` (Object Specification, the Component class table).
    pub const COMPONENT_USER_COMMENTS: u16 = 0x0204;
    /// `Component::Attributes` (Object Specification, the Component class table).
    pub const COMPONENT_ATTRIBUTES: u16 = 0x0205;
    /// `Event::Position` (Object Specification, the Event class table).
    pub const EVENT_POSITION: u16 = 0x0601;
    /// `Event::Comment` (Object Specification, the Event class table).
    pub const EVENT_COMMENT: u16 = 0x0602;
    /// `CommentMarker::Annotation` (Object Specification, the CommentMarker class table).
    pub const COMMENT_MARKER_ANNOTATION: u16 = 0x0901;
    /// `OperationGroup::Operation` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP_OPERATION: u16 = 0x0b01;
    /// `OperationGroup::InputSegments` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP_INPUT_SEGMENTS: u16 = 0x0b02;
    /// `OperationGroup::Parameters` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP_PARAMETERS: u16 = 0x0b03;
    /// `OperationGroup::BypassOverride` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP_BYPASS_OVERRIDE: u16 = 0x0b04;
    /// `OperationGroup::Rendering` (Object Specification, the OperationGroup class table).
    pub const OPERATION_GROUP_RENDERING: u16 = 0x0b05;
    /// `Sequence::Components` (Object Specification, the Sequence class table).
    pub const SEQUENCE_COMPONENTS: u16 = 0x1001;
    /// `SourceReference::SourceID` (Object Specification, the SourceReference class table).
    pub const SOURCE_REFERENCE_SOURCE_ID: u16 = 0x1101;
    /// `SourceReference::SourceMobSlotID` (Object Specification, the SourceReference class table).
    pub const SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID: u16 = 0x1102;
    /// `SourceReference::ChannelIDs` (Object Specification, the SourceReference class table).
    pub const SOURCE_REFERENCE_CHANNEL_IDS: u16 = 0x1103;
    /// `SourceReference::MonoSourceSlotIDs` (Object Specification, the SourceReference class table).
    pub const SOURCE_REFERENCE_MONO_SOURCE_SLOT_IDS: u16 = 0x1104;
    /// `SourceClip::StartTime` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP_START_TIME: u16 = 0x1201;
    /// `SourceClip::FadeInLength` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP_FADE_IN_LENGTH: u16 = 0x1202;
    /// `SourceClip::FadeInType` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP_FADE_IN_TYPE: u16 = 0x1203;
    /// `SourceClip::FadeOutLength` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP_FADE_OUT_LENGTH: u16 = 0x1204;
    /// `SourceClip::FadeOutType` (Object Specification, the SourceClip class table).
    pub const SOURCE_CLIP_FADE_OUT_TYPE: u16 = 0x1205;
    /// `Timecode::Start` (Object Specification, the Timecode class table).
    pub const TIMECODE_START: u16 = 0x1501;
    /// `Timecode::FPS` (Object Specification, the Timecode class table).
    pub const TIMECODE_FPS: u16 = 0x1502;
    /// `Timecode::Drop` (Object Specification, the Timecode class table).
    pub const TIMECODE_DROP: u16 = 0x1503;
    /// `Transition::OperationGroup` (Object Specification, the Transition class table).
    pub const TRANSITION_OPERATION_GROUP: u16 = 0x1801;
    /// `Transition::CutPoint` (Object Specification, the Transition class table).
    pub const TRANSITION_CUT_POINT: u16 = 0x1802;
    /// `ContentStorage::Mobs` (Object Specification, the ContentStorage class table).
    pub const CONTENT_STORAGE_MOBS: u16 = 0x1901;
    /// `ContentStorage::EssenceData` (Object Specification, the ContentStorage class table).
    pub const CONTENT_STORAGE_ESSENCE_DATA: u16 = 0x1902;
    /// `ControlPoint::Value` (Object Specification, the ControlPoint class table).
    pub const CONTROL_POINT_VALUE: u16 = 0x1a02;
    /// `ControlPoint::Time` (Object Specification, the ControlPoint class table).
    pub const CONTROL_POINT_TIME: u16 = 0x1a03;
    /// `ControlPoint::EditHint` (Object Specification, the ControlPoint class table).
    pub const CONTROL_POINT_EDIT_HINT: u16 = 0x1a04;
    /// `DefinitionObject::Identification` (Object Specification, the DefinitionObject class table).
    pub const DEFINITION_OBJECT_IDENTIFICATION: u16 = 0x1b01;
    /// `DefinitionObject::Name` (Object Specification, the DefinitionObject class table).
    pub const DEFINITION_OBJECT_NAME: u16 = 0x1b02;
    /// `DefinitionObject::Description` (Object Specification, the DefinitionObject class table).
    pub const DEFINITION_OBJECT_DESCRIPTION: u16 = 0x1b03;
    /// `OperationDefinition::DataDefinition` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_DATA_DEFINITION: u16 = 0x1e01;
    /// `OperationDefinition::IsTimeWarp` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_IS_TIME_WARP: u16 = 0x1e02;
    /// `OperationDefinition::DegradeTo` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_DEGRADE_TO: u16 = 0x1e03;
    /// `OperationDefinition::OperationCategory` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_OPERATION_CATEGORY: u16 = 0x1e06;
    /// `OperationDefinition::NumberInputs` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_NUMBER_INPUTS: u16 = 0x1e07;
    /// `OperationDefinition::Bypass` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_BYPASS: u16 = 0x1e08;
    /// `OperationDefinition::ParametersDefined` (Object Specification, the OperationDefinition class table).
    pub const OPERATION_DEFINITION_PARAMETERS_DEFINED: u16 = 0x1e09;
    /// `ParameterDefinition::Type` (Object Specification, the ParameterDefinition class table).
    pub const PARAMETER_DEFINITION_TYPE: u16 = 0x1f01;
    /// `ParameterDefinition::DisplayUnits` (Object Specification, the ParameterDefinition class table).
    pub const PARAMETER_DEFINITION_DISPLAY_UNITS: u16 = 0x1f03;
    /// `CodecDefinition::FileDescriptorClass` (Object Specification, the CodecDefinition class table).
    pub const CODEC_DEFINITION_FILE_DESCRIPTOR_CLASS: u16 = 0x2301;
    /// `CodecDefinition::DataDefinitions` (Object Specification, the CodecDefinition class table).
    pub const CODEC_DEFINITION_DATA_DEFINITIONS: u16 = 0x2302;
    /// `ContainerDefinition::EssenceIsIdentified` (Object Specification, the ContainerDefinition class table).
    pub const CONTAINER_DEFINITION_ESSENCE_IS_IDENTIFIED: u16 = 0x2401;
    /// `Dictionary::OperationDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_OPERATION_DEFINITIONS: u16 = 0x2603;
    /// `Dictionary::ParameterDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_PARAMETER_DEFINITIONS: u16 = 0x2604;
    /// `Dictionary::DataDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_DATA_DEFINITIONS: u16 = 0x2605;
    /// `Dictionary::PluginDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_PLUGIN_DEFINITIONS: u16 = 0x2606;
    /// `Dictionary::CodecDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_CODEC_DEFINITIONS: u16 = 0x2607;
    /// `Dictionary::ContainerDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_CONTAINER_DEFINITIONS: u16 = 0x2608;
    /// `Dictionary::InterpolationDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_INTERPOLATION_DEFINITIONS: u16 = 0x2609;
    /// `Dictionary::KLVDataDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_KLV_DATA_DEFINITIONS: u16 = 0x260a;
    /// `Dictionary::TaggedValueDefinitions` (Object Specification, the Dictionary class table).
    pub const DICTIONARY_TAGGED_VALUE_DEFINITIONS: u16 = 0x260b;
    /// `EssenceData::MobID` (Object Specification, the EssenceData class table).
    pub const ESSENCE_DATA_MOB_ID: u16 = 0x2701;
    /// `EssenceData::Data` (Object Specification, the EssenceData class table).
    pub const ESSENCE_DATA_DATA: u16 = 0x2702;
    /// `EssenceData::SampleIndex` (Object Specification, the EssenceData class table).
    pub const ESSENCE_DATA_SAMPLE_INDEX: u16 = 0x2b01;
    /// `EssenceDescriptor::Locator` (Object Specification, the EssenceDescriptor class table).
    pub const ESSENCE_DESCRIPTOR_LOCATOR: u16 = 0x2f01;
    /// `FileDescriptor::SampleRate` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR_SAMPLE_RATE: u16 = 0x3001;
    /// `FileDescriptor::Length` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR_LENGTH: u16 = 0x3002;
    /// `FileDescriptor::ContainerFormat` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR_CONTAINER_FORMAT: u16 = 0x3004;
    /// `FileDescriptor::CodecDefinition` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR_CODEC_DEFINITION: u16 = 0x3005;
    /// `FileDescriptor::LinkedSlotID` (Object Specification, the FileDescriptor class table).
    pub const FILE_DESCRIPTOR_LINKED_SLOT_ID: u16 = 0x3006;
    /// `AIFCDescriptor::Summary` (Object Specification, the AIFCDescriptor class table).
    pub const AIFC_DESCRIPTOR_SUMMARY: u16 = 0x3101;
    /// `WAVEDescriptor::Summary` (Object Specification, the WAVEDescriptor class table).
    pub const WAVE_DESCRIPTOR_SUMMARY: u16 = 0x3801;
    /// `Header::ByteOrder` (Object Specification, the Header class table).
    pub const HEADER_BYTE_ORDER: u16 = 0x3b01;
    /// `Header::LastModified` (Object Specification, the Header class table).
    pub const HEADER_LAST_MODIFIED: u16 = 0x3b02;
    /// `Header::Content` (Object Specification, the Header class table).
    pub const HEADER_CONTENT: u16 = 0x3b03;
    /// `Header::Dictionary` (Object Specification, the Header class table).
    pub const HEADER_DICTIONARY: u16 = 0x3b04;
    /// `Header::Version` (Object Specification, the Header class table).
    pub const HEADER_VERSION: u16 = 0x3b05;
    /// `Header::IdentificationList` (Object Specification, the Header class table).
    pub const HEADER_IDENTIFICATION_LIST: u16 = 0x3b06;
    /// `Header::ObjectModelVersion` (Object Specification, the Header class table).
    pub const HEADER_OBJECT_MODEL_VERSION: u16 = 0x3b07;
    /// `Header::PrimaryMob` (Object Specification, the Header class table).
    pub const HEADER_PRIMARY_MOB: u16 = 0x3b08;
    /// `Header::OperationalPattern` (Object Specification, the Header class table).
    pub const HEADER_OPERATIONAL_PATTERN: u16 = 0x3b09;
    /// `Header::EssenceContainers` (Object Specification, the Header class table).
    pub const HEADER_ESSENCE_CONTAINERS: u16 = 0x3b0a;
    /// `Header::DescriptiveSchemes` (Object Specification, the Header class table).
    pub const HEADER_DESCRIPTIVE_SCHEMES: u16 = 0x3b0b;
    /// `Identification::CompanyName` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_COMPANY_NAME: u16 = 0x3c01;
    /// `Identification::ProductName` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_PRODUCT_NAME: u16 = 0x3c02;
    /// `Identification::ProductVersion` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_PRODUCT_VERSION: u16 = 0x3c03;
    /// `Identification::ProductVersionString` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_PRODUCT_VERSION_STRING: u16 = 0x3c04;
    /// `Identification::ProductID` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_PRODUCT_ID: u16 = 0x3c05;
    /// `Identification::Date` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_DATE: u16 = 0x3c06;
    /// `Identification::ToolkitVersion` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_TOOLKIT_VERSION: u16 = 0x3c07;
    /// `Identification::Platform` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_PLATFORM: u16 = 0x3c08;
    /// `Identification::GenerationAUID` (Object Specification, the Identification class table).
    pub const IDENTIFICATION_GENERATION_AUID: u16 = 0x3c09;
    /// `NetworkLocator::URLString` (Object Specification, the NetworkLocator class table).
    pub const NETWORK_LOCATOR_URL_STRING: u16 = 0x4001;
    /// `Mob::MobID` (Object Specification, the Mob class table).
    pub const MOB_MOB_ID: u16 = 0x4401;
    /// `Mob::Name` (Object Specification, the Mob class table).
    pub const MOB_NAME: u16 = 0x4402;
    /// `Mob::Slots` (Object Specification, the Mob class table).
    pub const MOB_SLOTS: u16 = 0x4403;
    /// `Mob::LastModified` (Object Specification, the Mob class table).
    pub const MOB_LAST_MODIFIED: u16 = 0x4404;
    /// `Mob::CreationTime` (Object Specification, the Mob class table).
    pub const MOB_CREATION_TIME: u16 = 0x4405;
    /// `Mob::UserComments` (Object Specification, the Mob class table).
    pub const MOB_USER_COMMENTS: u16 = 0x4406;
    /// `Mob::KLVData` (Object Specification, the Mob class table).
    pub const MOB_KLV_DATA: u16 = 0x4407;
    /// `Mob::Attributes` (Object Specification, the Mob class table).
    pub const MOB_ATTRIBUTES: u16 = 0x4409;
    /// `Mob::UsageCode` (Object Specification, the Mob class table).
    pub const MOB_USAGE_CODE: u16 = 0x4408;
    /// `CompositionMob::DefaultFadeLength` (Object Specification, the CompositionMob class table).
    pub const COMPOSITION_MOB_DEFAULT_FADE_LENGTH: u16 = 0x4501;
    /// `CompositionMob::DefFadeType` (Object Specification, the CompositionMob class table).
    pub const COMPOSITION_MOB_DEF_FADE_TYPE: u16 = 0x4502;
    /// `CompositionMob::DefFadeEditUnit` (Object Specification, the CompositionMob class table).
    pub const COMPOSITION_MOB_DEF_FADE_EDIT_UNIT: u16 = 0x4503;
    /// `CompositionMob::Rendering` (Object Specification, the CompositionMob class table).
    pub const COMPOSITION_MOB_RENDERING: u16 = 0x4504;
    /// `SourceMob::EssenceDescription` (Object Specification, the SourceMob class table).
    pub const SOURCE_MOB_ESSENCE_DESCRIPTION: u16 = 0x4701;
    /// `MobSlot::SlotID` (Object Specification, the MobSlot class table).
    pub const MOB_SLOT_SLOT_ID: u16 = 0x4801;
    /// `MobSlot::SlotName` (Object Specification, the MobSlot class table).
    pub const MOB_SLOT_SLOT_NAME: u16 = 0x4802;
    /// `MobSlot::Segment` (Object Specification, the MobSlot class table).
    pub const MOB_SLOT_SEGMENT: u16 = 0x4803;
    /// `MobSlot::PhysicalTrackNumber` (Object Specification, the MobSlot class table).
    pub const MOB_SLOT_PHYSICAL_TRACK_NUMBER: u16 = 0x4804;
    /// `EventMobSlot::EditRate` (Object Specification, the EventMobSlot class table).
    pub const EVENT_MOB_SLOT_EDIT_RATE: u16 = 0x4901;
    /// `EventMobSlot::EventSlotOrigin` (Object Specification, the EventMobSlot class table).
    pub const EVENT_MOB_SLOT_EVENT_SLOT_ORIGIN: u16 = 0x4902;
    /// `TimelineMobSlot::EditRate` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT_EDIT_RATE: u16 = 0x4b01;
    /// `TimelineMobSlot::Origin` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT_ORIGIN: u16 = 0x4b02;
    /// `TimelineMobSlot::MarkIn` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT_MARK_IN: u16 = 0x4b03;
    /// `TimelineMobSlot::MarkOut` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT_MARK_OUT: u16 = 0x4b04;
    /// `TimelineMobSlot::UserPos` (Object Specification, the TimelineMobSlot class table).
    pub const TIMELINE_MOB_SLOT_USER_POS: u16 = 0x4b05;
    /// `Parameter::Definition` (Object Specification, the Parameter class table).
    pub const PARAMETER_DEFINITION: u16 = 0x4c01;
    /// `ConstantValue::Value` (Object Specification, the ConstantValue class table).
    pub const CONSTANT_VALUE_VALUE: u16 = 0x4d01;
    /// `VaryingValue::Interpolation` (Object Specification, the VaryingValue class table).
    pub const VARYING_VALUE_INTERPOLATION: u16 = 0x4e01;
    /// `VaryingValue::PointList` (Object Specification, the VaryingValue class table).
    pub const VARYING_VALUE_POINT_LIST: u16 = 0x4e02;
    /// `TaggedValue::Name` (Object Specification, the TaggedValue class table).
    pub const TAGGED_VALUE_NAME: u16 = 0x5001;
    /// `TaggedValue::Value` (Object Specification, the TaggedValue class table).
    pub const TAGGED_VALUE_VALUE: u16 = 0x5003;
    /// `DescriptiveMarker::DescribedSlots` (Object Specification, the DescriptiveMarker class table).
    pub const DESCRIPTIVE_MARKER_DESCRIBED_SLOTS: u16 = 0x6102;
    /// `DescriptiveMarker::Description` (Object Specification, the DescriptiveMarker class table).
    pub const DESCRIPTIVE_MARKER_DESCRIPTION: u16 = 0x6101;
    /// `SoundDescriptor::AudioSamplingRate` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_AUDIO_SAMPLING_RATE: u16 = 0x3d03;
    /// `SoundDescriptor::Locked` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_LOCKED: u16 = 0x3d02;
    /// `SoundDescriptor::AudioRefLevel` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_AUDIO_REF_LEVEL: u16 = 0x3d04;
    /// `SoundDescriptor::ElectroSpatial` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_ELECTRO_SPATIAL: u16 = 0x3d05;
    /// `SoundDescriptor::Channels` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_CHANNELS: u16 = 0x3d07;
    /// `SoundDescriptor::QuantizationBits` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_QUANTIZATION_BITS: u16 = 0x3d01;
    /// `SoundDescriptor::DialNorm` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_DIAL_NORM: u16 = 0x3d0c;
    /// `SoundDescriptor::Compression` (Object Specification, the SoundDescriptor class table).
    pub const SOUND_DESCRIPTOR_COMPRESSION: u16 = 0x3d06;
    /// `PCMDescriptor::BlockAlign` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_BLOCK_ALIGN: u16 = 0x3d0a;
    /// `PCMDescriptor::SequenceOffset` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_SEQUENCE_OFFSET: u16 = 0x3d0b;
    /// `PCMDescriptor::AverageBPS` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_AVERAGE_BPS: u16 = 0x3d09;
    /// `PCMDescriptor::ChannelAssignment` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_CHANNEL_ASSIGNMENT: u16 = 0x3d32;
    /// `PCMDescriptor::PeakEnvelopeVersion` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_ENVELOPE_VERSION: u16 = 0x3d29;
    /// `PCMDescriptor::PeakEnvelopeFormat` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_ENVELOPE_FORMAT: u16 = 0x3d2a;
    /// `PCMDescriptor::PointsPerPeakValue` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_POINTS_PER_PEAK_VALUE: u16 = 0x3d2b;
    /// `PCMDescriptor::PeakEnvelopeBlockSize` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_ENVELOPE_BLOCK_SIZE: u16 = 0x3d2c;
    /// `PCMDescriptor::PeakChannels` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_CHANNELS: u16 = 0x3d2d;
    /// `PCMDescriptor::PeakFrames` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_FRAMES: u16 = 0x3d2e;
    /// `PCMDescriptor::PeakOfPeaksPosition` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_OF_PEAKS_POSITION: u16 = 0x3d2f;
    /// `PCMDescriptor::PeakEnvelopeTimestamp` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_ENVELOPE_TIMESTAMP: u16 = 0x3d30;
    /// `PCMDescriptor::PeakEnvelopeData` (Object Specification, the PCMDescriptor class table).
    pub const PCM_DESCRIPTOR_PEAK_ENVELOPE_DATA: u16 = 0x3d31;
    /// `ClassDefinition::ParentClass` (Object Specification, the ClassDefinition class table).
    pub const CLASS_DEFINITION_PARENT_CLASS: u16 = 0x0008;
    /// `ClassDefinition::Properties` (Object Specification, the ClassDefinition class table).
    pub const CLASS_DEFINITION_PROPERTIES: u16 = 0x0009;
    /// `ClassDefinition::IsConcrete` (Object Specification, the ClassDefinition class table).
    pub const CLASS_DEFINITION_IS_CONCRETE: u16 = 0x000a;
    /// `PropertyDefinition::Type` (Object Specification, the PropertyDefinition class table).
    pub const PROPERTY_DEFINITION_TYPE: u16 = 0x000b;
    /// `PropertyDefinition::IsOptional` (Object Specification, the PropertyDefinition class table).
    pub const PROPERTY_DEFINITION_IS_OPTIONAL: u16 = 0x000c;
    /// `PropertyDefinition::LocalIdentification` (Object Specification, the PropertyDefinition class table).
    pub const PROPERTY_DEFINITION_LOCAL_IDENTIFICATION: u16 = 0x000d;
    /// `PropertyDefinition::IsUniqueIdentifier` (Object Specification, the PropertyDefinition class table).
    pub const PROPERTY_DEFINITION_IS_UNIQUE_IDENTIFIER: u16 = 0x000e;
    /// `MetaDefinition::Identification` (Object Specification, the MetaDefinition class table).
    pub const META_DEFINITION_IDENTIFICATION: u16 = 0x0005;
    /// `MetaDefinition::Name` (Object Specification, the MetaDefinition class table).
    pub const META_DEFINITION_NAME: u16 = 0x0006;
    /// `MetaDefinition::Description` (Object Specification, the MetaDefinition class table).
    pub const META_DEFINITION_DESCRIPTION: u16 = 0x0007;
    /// `MetaDictionary::ClassDefinitions` (Object Specification, the MetaDictionary class table).
    pub const META_DICTIONARY_CLASS_DEFINITIONS: u16 = 0x0003;
    /// `MetaDictionary::TypeDefinitions` (Object Specification, the MetaDictionary class table).
    pub const META_DICTIONARY_TYPE_DEFINITIONS: u16 = 0x0004;
}

/// Definition AUIDs (the plan's table, checked against the model tables
/// at <https://github.com/markreidvfx/pyaaf2>).
pub mod def {
    use super::Auid;

    /// `DataDef_Sound`.
    pub const DATA_DEF_SOUND: Auid = Auid::from_fields(
        0x01030202,
        0x0200,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x04, 0x01, 0x01, 0x01],
    );
    /// `DataDef_LegacySound` (read only).
    pub const DATA_DEF_LEGACY_SOUND: Auid = Auid::from_fields(
        0x78e1ebe1,
        0x6cef,
        0x11d2,
        [0x80, 0x7d, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `DataDef_Timecode`.
    pub const DATA_DEF_TIMECODE: Auid = Auid::from_fields(
        0x01030201,
        0x0100,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x04, 0x01, 0x01, 0x01],
    );
    /// `DataDef_LegacyTimecode` (read only).
    pub const DATA_DEF_LEGACY_TIMECODE: Auid = Auid::from_fields(
        0x7f275e81,
        0x77e5,
        0x11d2,
        [0x80, 0x7f, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `DataDef_DescriptiveMetadata`.
    pub const DATA_DEF_DESCRIPTIVE_METADATA: Auid = Auid::from_fields(
        0x01030201,
        0x1000,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x04, 0x01, 0x01, 0x01],
    );
    /// `OperationDef_MonoAudioDissolve`.
    pub const OPERATION_DEF_MONO_AUDIO_DISSOLVE: Auid = Auid::from_fields(
        0x0c3bea41,
        0xfc05,
        0x11d2,
        [0x8a, 0x29, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `OperationDef_MonoAudioGain`.
    pub const OPERATION_DEF_MONO_AUDIO_GAIN: Auid = Auid::from_fields(
        0x9d2ea894,
        0x0968,
        0x11d3,
        [0x8a, 0x38, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `OperationDef_MonoAudioPan`.
    pub const OPERATION_DEF_MONO_AUDIO_PAN: Auid = Auid::from_fields(
        0x9d2ea893,
        0x0968,
        0x11d3,
        [0x8a, 0x38, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `ParameterDef_Level`.
    pub const PARAMETER_DEF_LEVEL: Auid = Auid::from_fields(
        0xe4962320,
        0x2267,
        0x11d3,
        [0x8a, 0x4c, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `ParameterDef_Amplitude`.
    pub const PARAMETER_DEF_AMPLITUDE: Auid = Auid::from_fields(
        0xe4962321,
        0x2267,
        0x11d3,
        [0x8a, 0x4c, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `ParameterDef_Pan`.
    pub const PARAMETER_DEF_PAN: Auid = Auid::from_fields(
        0xe4962322,
        0x2267,
        0x11d3,
        [0x8a, 0x4c, 0x00, 0x50, 0x04, 0x0e, 0xf7, 0xd2],
    );
    /// `InterpolationDef_Linear`.
    pub const INTERPOLATION_DEF_LINEAR: Auid = Auid::from_fields(
        0x5b6c85a4,
        0x0ede,
        0x11d3,
        [0x80, 0xa9, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `InterpolationDef_Constant`.
    pub const INTERPOLATION_DEF_CONSTANT: Auid = Auid::from_fields(
        0x5b6c85a5,
        0x0ede,
        0x11d3,
        [0x80, 0xa9, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `InterpolationDef_BSpline`.
    pub const INTERPOLATION_DEF_B_SPLINE: Auid = Auid::from_fields(
        0x5b6c85a6,
        0x0ede,
        0x11d3,
        [0x80, 0xa9, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `InterpolationDef_Log`.
    pub const INTERPOLATION_DEF_LOG: Auid = Auid::from_fields(
        0x15829ec3,
        0x1f24,
        0x458a,
        [0x96, 0x0d, 0xc6, 0x5b, 0xb2, 0x3c, 0x2a, 0xa1],
    );
    /// `InterpolationDef_Power`.
    pub const INTERPOLATION_DEF_POWER: Auid = Auid::from_fields(
        0xc09153f7,
        0xbd18,
        0x4e5a,
        [0xad, 0x09, 0xcb, 0xdd, 0x65, 0x4f, 0xa0, 0x01],
    );
    /// `ContainerDef_AAF`.
    pub const CONTAINER_DEF_AAF: Auid = Auid::from_fields(
        0x4313b571,
        0xd8ba,
        0x11d2,
        [0x80, 0x9b, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `ContainerDef_External`.
    pub const CONTAINER_DEF_EXTERNAL: Auid = Auid::from_fields(
        0x4313b572,
        0xd8ba,
        0x11d2,
        [0x80, 0x9b, 0x00, 0x60, 0x08, 0x14, 0x3e, 0x6f],
    );
    /// `OPDef_EditProtocol`.
    pub const OP_DEF_EDIT_PROTOCOL: Auid = Auid::from_fields(
        0x0d011201,
        0x0100,
        0x0000,
        [0x06, 0x0e, 0x2b, 0x34, 0x04, 0x01, 0x01, 0x05],
    );
}

/// The bytes of `p` when it is data.
fn data(p: &Prop) -> Option<&[u8]> {
    match p {
        Prop::Data(d) => Some(d),
        _ => None,
    }
}

/// An `Int64` (positions, lengths), or an `Int32` widened; anything else
/// is `None`.
pub fn i64_at(p: &Prop) -> Option<i64> {
    let d = data(p)?;
    if let Ok(b) = <[u8; 8]>::try_from(d) {
        return Some(i64::from_le_bytes(b));
    }
    <[u8; 4]>::try_from(d)
        .ok()
        .map(|b| i64::from(i32::from_le_bytes(b)))
}

/// A `Rational` (numerator, denominator); a denominator of zero or less is
/// `None`.
pub fn rational(p: &Prop) -> Option<(i32, i32)> {
    let d = <[u8; 8]>::try_from(data(p)?).ok()?;
    let [a, b, c, e, f, g, h, i] = d;
    let num = i32::from_le_bytes([a, b, c, e]);
    let den = i32::from_le_bytes([f, g, h, i]);
    (den > 0).then_some((num, den))
}

/// An `AUID` (16 bytes as stored).
pub fn auid(p: &Prop) -> Option<Auid> {
    <[u8; 16]>::try_from(data(p)?).ok().map(Auid)
}

/// A `MobIDType` (32 bytes as stored).
pub fn mob_id(p: &Prop) -> Option<MobId> {
    <[u8; 32]>::try_from(data(p)?).ok().map(MobId)
}

/// A `String`: UTF-16LE, with or without its NUL (text after a NUL is
/// dropped), an odd length dropping its last byte and unpaired surrogates
/// becoming U+FFFD; then [`crate::text::sanitize`] at
/// `limits.max_text_bytes`. The flag is true when the text was cut or
/// changed. Not data is an empty, flagged string.
pub fn string(p: &Prop, limits: &AafLimits) -> (String, bool) {
    let Some(d) = data(p) else {
        return (String::new(), true);
    };
    let mut changed = d.len() % 2 == 1;
    let mut units = d.chunks_exact(2).map(|c| {
        u16::from_le_bytes([
            c.first().copied().unwrap_or(0),
            c.get(1).copied().unwrap_or(0),
        ])
    });
    let mut decoded = String::new();
    // Work stops a little past the limit: sanitize cuts there anyway, and
    // the extra character makes it see the cut.
    let room = limits.max_text_bytes.saturating_add(4);
    let mut nul = false;
    for c in char::decode_utf16(units.by_ref().take_while(|&u| {
        nul = u == 0;
        !nul
    })) {
        if decoded.len() >= room {
            changed = true;
            break;
        }
        decoded.push(c.unwrap_or_else(|_| {
            changed = true;
            '\u{FFFD}'
        }));
    }
    if nul && units.any(|u| u != 0) {
        changed = true;
    }
    let lim = Limits {
        max_text_bytes: limits.max_text_bytes,
        ..Limits::default()
    };
    let (out, cut) = crate::text::sanitize(decoded.as_bytes(), &lim);
    let changed = changed || cut || out != decoded;
    (out, changed)
}
