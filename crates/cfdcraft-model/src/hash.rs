use crate::ResolvedStudy;

/// Hash an SI recipe from [`crate::resolve`] using RFC 8785 JSON and BLAKE3.
/// JCS is a hashing format only; store studies with ordinary pretty JSON.
pub fn canonical_hash(study: &ResolvedStudy) -> String {
    let canonical =
        serde_jcs::to_string(study).expect("a resolved study contains finite SI quantities only");
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}
