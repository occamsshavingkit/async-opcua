//! Hand-authored GDS `ApplicationRecordDataType` and its [`TypeLoader`].
//!
//! OPC UA Part 12 defines this companion type, but the vendored GDS NodeSet does not currently
//! produce a generated Rust binding. Its DataType NodeId (`ns=1;i=1`) is reused as the encoding
//! identifier because that NodeSet does not define separate encoding nodes.

use std::io::{Read, Write};

#[allow(unused)]
mod opcua {
    pub(super) use crate as types;
}

use crate::{
    ApplicationType, BinaryDecodable, BinaryEncodable, Context, DynEncodable, EncodingResult,
    ExpandedMessageInfo, ExpandedNodeId, LocalizedText, NodeId, TypeLoader, TypeLoaderInstance,
    UAString,
};

/// The GDS companion namespace URI.
pub const GDS_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/GDS/";

/// The `ApplicationRecordDataType` identifier in the GDS companion NodeSet.
const APPLICATION_RECORD_DATA_TYPE_ID: u32 = 1;

/// OPC-10000-12 §6.5.5 wire representation used by the GDS application registry methods.
#[derive(Debug, Clone, PartialEq, Default, crate::UaNullable)]
#[cfg_attr(feature = "json", derive(crate::JsonEncodable, crate::JsonDecodable))]
#[cfg_attr(
    feature = "xml",
    derive(crate::XmlEncodable, crate::XmlDecodable, crate::XmlType)
)]
pub struct ApplicationRecordDataType {
    /// The unique identifier assigned by the GDS, or null for a new registration.
    pub application_id: NodeId,
    /// The URI for the application associated with the record.
    pub application_uri: UAString,
    /// The type of application.
    pub application_type: ApplicationType,
    /// One or more localized names for the application.
    pub application_names: Option<Vec<LocalizedText>>,
    /// A globally unique URI for the product associated with the application.
    pub product_uri: UAString,
    /// The discovery URLs for the application.
    pub discovery_urls: Option<Vec<UAString>>,
    /// The server capability identifiers for the application.
    pub server_capabilities: Option<Vec<UAString>>,
}

impl BinaryEncodable for ApplicationRecordDataType {
    fn byte_len(&self, ctx: &Context<'_>) -> usize {
        let mut size = 0usize;
        size += BinaryEncodable::byte_len(&self.application_id, ctx);
        size += BinaryEncodable::byte_len(&self.application_uri, ctx);
        size += BinaryEncodable::byte_len(&self.application_type, ctx);
        size += BinaryEncodable::byte_len(&self.application_names, ctx);
        size += BinaryEncodable::byte_len(&self.product_uri, ctx);
        size += BinaryEncodable::byte_len(&self.discovery_urls, ctx);
        size += BinaryEncodable::byte_len(&self.server_capabilities, ctx);
        size
    }

    fn encode<S: Write + ?Sized>(&self, stream: &mut S, ctx: &Context<'_>) -> EncodingResult<()> {
        BinaryEncodable::encode(&self.application_id, stream, ctx)?;
        BinaryEncodable::encode(&self.application_uri, stream, ctx)?;
        BinaryEncodable::encode(&self.application_type, stream, ctx)?;
        BinaryEncodable::encode(&self.application_names, stream, ctx)?;
        BinaryEncodable::encode(&self.product_uri, stream, ctx)?;
        BinaryEncodable::encode(&self.discovery_urls, stream, ctx)?;
        BinaryEncodable::encode(&self.server_capabilities, stream, ctx)?;
        Ok(())
    }
}

impl BinaryDecodable for ApplicationRecordDataType {
    fn decode<S: Read + ?Sized>(stream: &mut S, ctx: &Context<'_>) -> EncodingResult<Self> {
        Ok(Self {
            application_id: BinaryDecodable::decode(stream, ctx)?,
            application_uri: BinaryDecodable::decode(stream, ctx)?,
            application_type: BinaryDecodable::decode(stream, ctx)?,
            application_names: BinaryDecodable::decode(stream, ctx)?,
            product_uri: BinaryDecodable::decode(stream, ctx)?,
            discovery_urls: BinaryDecodable::decode(stream, ctx)?,
            server_capabilities: BinaryDecodable::decode(stream, ctx)?,
        })
    }
}

impl ExpandedMessageInfo for ApplicationRecordDataType {
    fn full_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::new_with_namespace(GDS_NAMESPACE_URI, APPLICATION_RECORD_DATA_TYPE_ID)
    }

    fn full_json_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::new_with_namespace(GDS_NAMESPACE_URI, APPLICATION_RECORD_DATA_TYPE_ID)
    }

    fn full_xml_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::new_with_namespace(GDS_NAMESPACE_URI, APPLICATION_RECORD_DATA_TYPE_ID)
    }

    fn full_data_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::new_with_namespace(GDS_NAMESPACE_URI, APPLICATION_RECORD_DATA_TYPE_ID)
    }
}

/// Resolves GDS `ApplicationRecordDataType` extension objects.
#[derive(Debug, Clone, Copy, Default)]
pub struct GdsApplicationRecordTypeLoader;

fn types() -> TypeLoaderInstance {
    let mut inst = TypeLoaderInstance::new();
    inst.add_binary_type(
        APPLICATION_RECORD_DATA_TYPE_ID,
        APPLICATION_RECORD_DATA_TYPE_ID,
        crate::binary_decode_to_enc::<ApplicationRecordDataType>,
    );
    #[cfg(feature = "xml")]
    inst.add_xml_type(
        APPLICATION_RECORD_DATA_TYPE_ID,
        APPLICATION_RECORD_DATA_TYPE_ID,
        crate::xml_decode_to_enc::<ApplicationRecordDataType>,
    );
    #[cfg(feature = "json")]
    inst.add_json_type(
        APPLICATION_RECORD_DATA_TYPE_ID,
        APPLICATION_RECORD_DATA_TYPE_ID,
        crate::json_decode_to_enc::<ApplicationRecordDataType>,
    );
    inst
}

impl TypeLoader for GdsApplicationRecordTypeLoader {
    fn load_from_binary(
        &self,
        node_id: &NodeId,
        stream: &mut dyn Read,
        ctx: &Context<'_>,
        _length: Option<usize>,
    ) -> Option<EncodingResult<Box<dyn DynEncodable>>> {
        let idx = ctx.namespaces().get_index(GDS_NAMESPACE_URI)?;
        if idx != node_id.namespace {
            return None;
        }
        let num_id = node_id.as_u32()?;
        types().decode_binary(num_id, stream, ctx)
    }

    #[cfg(feature = "xml")]
    fn load_from_xml(
        &self,
        node_id: &NodeId,
        stream: &mut crate::xml::XmlStreamReader<&mut dyn Read>,
        ctx: &Context<'_>,
        _name: &str,
    ) -> Option<EncodingResult<Box<dyn DynEncodable>>> {
        let idx = ctx.namespaces().get_index(GDS_NAMESPACE_URI)?;
        if idx != node_id.namespace {
            return None;
        }
        let num_id = node_id.as_u32()?;
        types().decode_xml(num_id, stream, ctx)
    }

    #[cfg(feature = "json")]
    fn load_from_json(
        &self,
        node_id: &NodeId,
        stream: &mut crate::json::JsonStreamReader<&mut dyn Read>,
        ctx: &Context<'_>,
    ) -> Option<EncodingResult<Box<dyn DynEncodable>>> {
        let idx = ctx.namespaces().get_index(GDS_NAMESPACE_URI)?;
        if idx != node_id.namespace {
            return None;
        }
        let num_id = node_id.as_u32()?;
        types().decode_json(num_id, stream, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwned, DecodingOptions, ExtensionObject, NamespaceMap};

    fn test_context() -> (NamespaceMap, ContextOwned) {
        let mut namespaces = NamespaceMap::new();
        namespaces.add_namespace(GDS_NAMESPACE_URI);
        let mut ctx = ContextOwned::new_default(
            namespaces.clone(),
            std::sync::Arc::new(DecodingOptions::default()),
        );
        ctx.loaders_mut()
            .add_type_loader(GdsApplicationRecordTypeLoader);
        (namespaces, ctx)
    }

    fn sample() -> ApplicationRecordDataType {
        let (namespaces, _ctx) = test_context();
        let ns = namespaces.get_index(GDS_NAMESPACE_URI).unwrap();
        ApplicationRecordDataType {
            application_id: NodeId::new(ns, "Application.1"),
            application_uri: UAString::from("urn:example:app"),
            application_type: ApplicationType::Client,
            application_names: Some(vec![LocalizedText::from("Example App")]),
            product_uri: UAString::from("urn:example:products:app"),
            discovery_urls: Some(vec![UAString::from("opc.tcp://example:4840")]),
            server_capabilities: Some(vec![UAString::from("NA")]),
        }
    }

    #[test]
    fn round_trips_through_extension_object_via_the_type_loader() {
        let (_namespaces, ctx_owned) = test_context();
        let ctx = ctx_owned.context();

        let value = sample();
        let obj = ExtensionObject::new(value.clone());

        let mut bytes = Vec::new();
        BinaryEncodable::encode(&obj, &mut bytes, &ctx).expect("encode should succeed");

        let mut cursor = std::io::Cursor::new(bytes);
        let decoded: ExtensionObject =
            BinaryDecodable::decode(&mut cursor, &ctx).expect("decode should succeed");

        let decoded = decoded
            .into_inner_as::<ApplicationRecordDataType>()
            .expect("should decode back into ApplicationRecordDataType");
        assert_eq!(*decoded, value);
    }
}
