use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use opcua_crypto::{CertificateStore, PrivateKey, X509};
use opcua_types::{ByteString, Error, StatusCode};

#[async_trait]
/// Source for an issued token. Since each re-authentication when using
/// issued tokens may require a new token.
pub trait IssuedTokenSource: Send + Sync {
    /// Get a valid issued token. This may be a cached token,
    /// or a new one if the cache is empty or expired.
    async fn get_issued_token(&self) -> Result<ByteString, Error>;
}

#[async_trait]
impl IssuedTokenSource for ByteString {
    async fn get_issued_token(&self) -> Result<ByteString, Error> {
        Ok(self.clone())
    }
}

/// Wrapper for an issued token source.
#[derive(Clone)]
pub struct IssuedTokenWrapper(pub(crate) Arc<dyn IssuedTokenSource>);

impl IssuedTokenWrapper {
    /// Create a new issued token wrapper from a reference to an issued token source.
    pub fn new(token_source: Arc<dyn IssuedTokenSource>) -> Self {
        Self(token_source)
    }

    /// Create a new issued token wrapper.
    pub fn new_source(token_source: impl IssuedTokenSource + 'static) -> Self {
        Self(Arc::new(token_source))
    }
}

impl std::fmt::Debug for IssuedTokenWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IssuedTokenSource").finish()
    }
}

#[derive(Clone)]
/// A wrapper around a password as string. This intentionally implements
/// debug in a way that does not expose the password.
pub struct Password(pub String);

impl Password {
    /// Create a new password from a string.
    pub fn new(password: impl Into<String>) -> Self {
        Password(password.into())
    }
}

impl<T> From<T> for Password
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Password(value.into())
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Password").field(&"*****").finish()
    }
}

#[derive(Debug, Clone)]
/// Client-side identity token representation.
pub enum IdentityToken {
    /// Anonymous identity token
    Anonymous,
    /// User name and a password
    UserName(String, Password),
    /// X5090 cert and private key.
    X509(Box<X509>, Box<PrivateKey>),
    /// Issued token
    IssuedToken(IssuedTokenWrapper),
}

impl IdentityToken {
    /// Create a new anonymous identity token.
    pub fn new_anonymous() -> Self {
        IdentityToken::Anonymous
    }
    /// Create a new user name identity token.
    pub fn new_user_name(user_name: impl Into<String>, password: impl Into<Password>) -> Self {
        IdentityToken::UserName(user_name.into(), password.into())
    }

    /// Create a new x509 identity token.
    pub fn new_x509(cert: X509, private_key: PrivateKey) -> Self {
        IdentityToken::X509(Box::new(cert), Box::new(private_key))
    }

    /// Create a new x509 identity token from a path to a certificate and private key.
    pub fn new_x509_path(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, Error> {
        let cert = CertificateStore::read_cert(cert_path.as_ref())
            .map_err(|e| Error::new(StatusCode::Bad, e))?;
        let private_key = CertificateStore::read_pkey(key_path.as_ref())
            .map_err(|e| Error::new(StatusCode::Bad, e))?;
        Ok(IdentityToken::X509(Box::new(cert), Box::new(private_key)))
    }

    /// Create a new issued token based identity token.
    pub fn new_issued_token(token_source: impl IssuedTokenSource + 'static) -> Self {
        IdentityToken::IssuedToken(IssuedTokenWrapper::new_source(token_source))
    }

    /// Create a new issued token based identity token from a shared reference.
    pub fn new_issued_token_arc(token_source: Arc<dyn IssuedTokenSource>) -> Self {
        IdentityToken::IssuedToken(IssuedTokenWrapper::new(token_source))
    }

    /// Create a Kerberos IssuedToken identity for the given service principal (OPC 10000-6 §6.4).
    /// Acquires a GSSAPI service ticket and wraps it as a `GSSAPI <base64>` IssuedToken.
    /// Requires the `kerberos` feature and a valid Kerberos TGT.
    #[cfg(feature = "kerberos")]
    pub fn new_kerberos(spn: impl Into<String>) -> Result<Self, opcua_types::Error> {
        let token = acquire_kerberos_token(&spn.into())
            .map_err(|e| opcua_types::Error::new(StatusCode::BadIdentityTokenRejected, e))?;
        Ok(IdentityToken::IssuedToken(IssuedTokenWrapper::new_source(
            token,
        )))
    }
}

/// Acquire a Kerberos service ticket via GSSAPI for the given SPN (OPC 10000-6 §6.4).
/// Returns the token as a `ByteString` with the `GSSAPI ` base64 prefix.
#[cfg(feature = "kerberos")]
pub fn acquire_kerberos_token(spn: &str) -> Result<ByteString, String> {
    use base64::Engine;
    use libgssapi::context::ClientCtx;
    use libgssapi::credential::{Cred, CredUsage};
    use libgssapi::name::Name;
    use libgssapi::oid::{GSS_MECH_KRB5, GSS_NT_HOSTBASED_SERVICE};

    let name = Name::new(spn.as_bytes(), Some(GSS_NT_HOSTBASED_SERVICE))
        .map_err(|_| format!("GSSAPI: invalid SPN '{spn}'"))?;
    let canonical = name
        .canonicalize(Some(GSS_MECH_KRB5))
        .map_err(|_| "GSSAPI: failed to canonicalize SPN".to_string())?;

    let cred = Cred::acquire(
        None,
        None,
        CredUsage::Initiate,
        None::<&libgssapi::oid::OidSet>,
    )
    .map_err(|_| "GSSAPI: no Kerberos credentials (run kinit)".to_string())?;

    let mut client = ClientCtx::new(
        Some(cred),
        canonical,
        libgssapi::context::CtxFlags::GSS_C_MUTUAL_FLAG
            | libgssapi::context::CtxFlags::GSS_C_CONF_FLAG,
        Some(GSS_MECH_KRB5),
    );

    let token = client
        .step(None, None)
        .map_err(|_| "GSSAPI: failed to acquire service ticket".to_string())?;

    let token_bytes = token.map(|b| b.to_vec()).unwrap_or_default();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&token_bytes);
    Ok(ByteString::from(format!("GSSAPI {encoded}").into_bytes()))
}
