use opcua_types::{AccessRestrictionType, DateTime, IdType, NumericRange, RolePermissionType};

/// Namespace metadata exposed in the namespace array and used by node managers.
#[derive(Default, Clone, Debug)]
pub struct NamespaceMetadata {
    /// Default access restrictions on this namespace.
    pub default_access_restrictions: AccessRestrictionType,
    /// Default role permissions on this namespace.
    pub default_role_permissions: Option<Vec<RolePermissionType>>,
    /// Default user role permissions on this namespace.
    pub default_user_role_permissions: Option<Vec<RolePermissionType>>,
    /// Whether this namespace is a subset of the full namespace.
    pub is_namespace_subset: Option<bool>,
    /// Time this namespace was last updated.
    pub namespace_publication_date: Option<DateTime>,
    /// Namespace URI.
    pub namespace_uri: String,
    /// Namespace version.
    pub namespace_version: Option<String>,
    /// List of ID types in this namespace.
    pub static_node_id_types: Option<Vec<IdType>>,
    /// List of ranges for numeric node IDs on static nodes.
    pub static_numeric_node_id_range: Option<Vec<NumericRange>>,
    /// Pattern that applies to string node IDs on static nodes.
    pub static_string_node_id_pattern: Option<String>,
    /// Namespace index on the server.
    pub namespace_index: u16,
}
