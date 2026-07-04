use std::io::Read;

use tracing::{debug, trace, warn};

use super::config::{OcspError, OcspFetchConfig};

const OCSP_REQUEST_CONTENT_TYPE: &str = "application/ocsp-request";
const OCSP_RESPONSE_CONTENT_TYPE: &str = "application/ocsp-response";

pub fn fetch_ocsp_response(
    url: &str,
    request_der: &[u8],
    config: &OcspFetchConfig,
) -> Result<Vec<u8>, OcspError> {
    trace!("Fetching OCSP response from {url}");

    let response = ureq::Agent::new_with_defaults()
        .post(url)
        .header("Content-Type", OCSP_REQUEST_CONTENT_TYPE)
        .header("Accept", OCSP_RESPONSE_CONTENT_TYPE)
        .send(request_der)
        .map_err(|e| {
            let msg = format!("OCSP HTTP POST to {url} failed: {e}");
            warn!("{msg}");
            OcspError::FetchFailed(msg)
        })?;

    let content_type = response
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>")
        .to_string();
    debug!("OCSP response Content-Type: {content_type}");

    let max_size = config.max_response_size;
    let mut body = Vec::new();
    response
        .into_body()
        .as_reader()
        .take(max_size as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| OcspError::FetchFailed(format!("failed to read OCSP response body: {e}")))?;

    if body.len() > max_size {
        return Err(OcspError::FetchFailed(format!(
            "OCSP response body exceeds maximum size of {max_size} bytes"
        )));
    }

    trace!("Fetched OCSP response: {} bytes", body.len());
    Ok(body)
}
