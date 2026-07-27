//! Continuation-point pagination for annotation history reads.

use std::ops::Range;

use opcua_types::StatusCode;

const BATCH_LIMIT: usize = 1_000;

/// Opaque annotation-history continuation state encoded as a requested-item offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnnotationContinuationPoint {
    offset: usize,
}

impl AnnotationContinuationPoint {
    /// Decodes an optional continuation token and validates it against the request item count.
    pub fn decode(token: Option<&[u8]>, item_count: usize) -> Result<Self, StatusCode> {
        let offset = match token {
            Some(token) => {
                let bytes: [u8; 8] = token
                    .try_into()
                    .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
                usize::try_from(u64::from_le_bytes(bytes))
                    .map_err(|_| StatusCode::BadContinuationPointInvalid)?
            }
            None => 0,
        };

        if offset > item_count {
            return Err(StatusCode::BadContinuationPointInvalid);
        }

        Ok(Self { offset })
    }

    /// Returns the current page range and an opaque token for the following page, when needed.
    pub fn page(self, item_count: usize) -> (Range<usize>, Option<Vec<u8>>) {
        let page_end = self.offset.saturating_add(BATCH_LIMIT).min(item_count);
        let next_token = (page_end < item_count).then(|| (page_end as u64).to_le_bytes().to_vec());
        (self.offset..page_end, next_token)
    }
}
