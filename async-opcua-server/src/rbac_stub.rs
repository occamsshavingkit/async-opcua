//! Minimal RBAC compatibility surface for builds without the `rbac` feature.

use opcua_types::{NodeId, ObjectId};

/// Role resolver types.
pub(crate) mod resolver {
    use opcua_types::NodeId;

    /// Activated session identity used by the auth spine.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ResolvedIdentity;

    impl ResolvedIdentity {
        /// Creates an anonymous resolved identity.
        pub(crate) fn anonymous(
            _application_uri: Option<impl Into<String>>,
            _endpoint_url: Option<impl Into<String>>,
        ) -> Self {
            Self
        }

        /// Creates a username resolved identity.
        pub(crate) fn username(
            _username: impl Into<String>,
            _application_uri: Option<impl Into<String>>,
            _endpoint_url: Option<impl Into<String>>,
        ) -> Self {
            Self
        }

        /// Creates an X.509 thumbprint resolved identity.
        pub(crate) fn x509_thumbprint(
            _thumbprint: impl Into<String>,
            _application_uri: Option<impl Into<String>>,
            _endpoint_url: Option<impl Into<String>>,
        ) -> Self {
            Self
        }

        /// Creates an issued-token resolved identity.
        pub(crate) fn issued_token<GroupIds, GroupId, RoleIds>(
            _group_ids: GroupIds,
            _role_ids: RoleIds,
            _application_uri: Option<impl Into<String>>,
            _endpoint_url: Option<impl Into<String>>,
        ) -> Self
        where
            GroupIds: IntoIterator<Item = GroupId>,
            GroupId: Into<String>,
            RoleIds: IntoIterator<Item = NodeId>,
        {
            Self
        }
    }

    /// Zero-size resolver used when RBAC is compiled out.
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) struct RoleResolver;

    impl RoleResolver {
        /// Resolves every identity to no roles.
        pub(crate) fn resolve(&self, _identity: &ResolvedIdentity) -> Vec<NodeId> {
            Vec::new()
        }
    }
}

/// Namespace-default RBAC attributes.
pub(crate) mod defaults {
    use std::collections::BTreeMap;

    use opcua_types::{AccessRestrictionType, RolePermissionType};

    use crate::config::NamespaceDefaultConfig;

    /// Empty namespace defaults used when RBAC is compiled out.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct NamespaceDefaults;

    impl NamespaceDefaults {
        /// Ignores configured RBAC defaults when RBAC is compiled out.
        pub(crate) fn from_config(
            _config_defaults: &BTreeMap<u16, NamespaceDefaultConfig>,
        ) -> Self {
            Self
        }

        /// Returns no role permissions.
        #[cfg_attr(
            not(any(feature = "generated-address-space", feature = "diagnostics")),
            allow(dead_code)
        )]
        pub(crate) fn role_permissions(&self, _namespace: u16) -> Option<&[RolePermissionType]> {
            None
        }

        /// Returns no access restrictions.
        #[cfg_attr(
            not(any(feature = "generated-address-space", feature = "diagnostics")),
            allow(dead_code)
        )]
        pub(crate) fn access_restrictions(&self, _namespace: u16) -> Option<AccessRestrictionType> {
            None
        }
    }
}

/// Authorization decisions.
pub(crate) mod decision {
    use crate::{address_space::NodeType, node_manager::RequestContext};
    use opcua_types::{AttributeId, NodeId, PermissionType, RolePermissionType, StatusCode};

    /// Context-taking wrapper for node authorization decisions.
    #[must_use]
    pub(crate) fn authorize_ctx(
        context: &RequestContext,
        _node: &NodeType,
        _required: PermissionType,
    ) -> bool {
        !context.enforce_role_based_access()
    }

    /// Context-taking wrapper for AccessRestrictions decisions.
    pub(crate) fn access_restrictions_ok_ctx(
        context: &RequestContext,
        _node: &NodeType,
    ) -> Result<(), StatusCode> {
        if context.enforce_role_based_access() {
            Err(StatusCode::BadUserAccessDenied)
        } else {
            Ok(())
        }
    }

    /// Returns whether a session may receive Events from an event source.
    #[must_use]
    #[cfg_attr(not(feature = "events"), allow(dead_code))]
    pub(crate) fn event_receive_allowed_with_enforcement(
        _user_roles: &[NodeId],
        _source_role_permissions: Option<&[RolePermissionType]>,
        enforce_role_based_access: bool,
    ) -> bool {
        !enforce_role_based_access
    }

    /// Permission required to write an attribute with the Write service.
    #[must_use]
    pub(crate) fn permission_for_write_attribute(attribute_id: AttributeId) -> PermissionType {
        match attribute_id {
            AttributeId::Value => PermissionType::Write,
            AttributeId::RolePermissions => PermissionType::WriteRolePermissions,
            AttributeId::Historizing => PermissionType::WriteHistorizing,
            _ => PermissionType::WriteAttribute,
        }
    }
}

/// Identity mapping rules are not applied without the `rbac` feature.
pub(crate) mod rules {}

/// Standard OPC UA well-known roles used by RoleSet.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WellKnownRole {
    /// Anonymous role.
    Anonymous,
    /// AuthenticatedUser role.
    AuthenticatedUser,
    /// Observer role.
    Observer,
    /// Operator role.
    Operator,
    /// Engineer role.
    Engineer,
    /// Supervisor role.
    Supervisor,
    /// ConfigureAdmin role.
    ConfigureAdmin,
    /// SecurityAdmin role.
    SecurityAdmin,
}

impl WellKnownRole {
    /// Returns the standard namespace 0 NodeId for this well-known role.
    #[cfg_attr(not(feature = "gds"), allow(dead_code))]
    pub(crate) fn node_id(self) -> NodeId {
        match self {
            Self::Anonymous => ObjectId::WellKnownRole_Anonymous.into(),
            Self::AuthenticatedUser => ObjectId::WellKnownRole_AuthenticatedUser.into(),
            Self::Observer => ObjectId::WellKnownRole_Observer.into(),
            Self::Operator => ObjectId::WellKnownRole_Operator.into(),
            Self::Engineer => ObjectId::WellKnownRole_Engineer.into(),
            Self::Supervisor => ObjectId::WellKnownRole_Supervisor.into(),
            Self::ConfigureAdmin => ObjectId::WellKnownRole_ConfigureAdmin.into(),
            Self::SecurityAdmin => ObjectId::WellKnownRole_SecurityAdmin.into(),
        }
    }
}
