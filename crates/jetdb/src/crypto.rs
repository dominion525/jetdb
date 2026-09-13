//! Encryption support for .accdb files (MS-OFFCRYPTO).
//!
//! Supports Agile Encryption, RC4 CryptoAPI, and NonStandard AES.
//! The EncryptionInfo is stored at page 0 offset 0x299.

use crate::file::FileError;
use crate::format::db_header;

use crate::file::rc4_transform;

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyInit, KeyIvInit};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes192CbcDec = cbc::Decryptor<aes::Aes192>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type Aes128EcbDec = ecb::Decryptor<aes::Aes128>;
type Aes192EcbDec = ecb::Decryptor<aes::Aes192>;
type Aes256EcbDec = ecb::Decryptor<aes::Aes256>;

// ---------------------------------------------------------------------------
// EncryptionInfo flags (MS-OFFCRYPTO §2.3.2)
// ---------------------------------------------------------------------------

const FCRYPTO_API_FLAG: u32 = 0x04;
const FAES_FLAG: u32 = 0x20;

// CryptoAlgorithm IDs (MS-OFFCRYPTO §2.3.2)
const ALGID_RC4: u32 = 0x6801;
const ALGID_AES_128: u32 = 0x660E;
const ALGID_AES_192: u32 = 0x660F;
const ALGID_AES_256: u32 = 0x6610;

// ---------------------------------------------------------------------------
// Block key constants for password key encryptor
// ---------------------------------------------------------------------------

const VERIFIER_HASH_INPUT_BLOCK_KEY: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_HASH_VALUE_BLOCK_KEY: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const ENCRYPTED_KEY_VALUE_BLOCK_KEY: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];

/// Padding byte for key derivation and IV construction (MS-OFFCRYPTO §2.3.6.2).
const HASH_PAD_BYTE: u8 = 0x36;

// ---------------------------------------------------------------------------
// AgileParams
// ---------------------------------------------------------------------------

/// Parameters parsed from the EncryptionInfo XML.
#[derive(Debug, Clone)]
pub(crate) struct AgileParams {
    // Key data (for page decryption)
    pub key_bits: usize,
    pub block_size: usize,
    pub hash_algorithm: HashAlgorithm,
    pub salt_value: Vec<u8>,

    // Password key encryptor
    pub pe_spin_count: u32,
    pub pe_salt_value: Vec<u8>,
    pub pe_hash_algorithm: HashAlgorithm,
    pub pe_key_bits: usize,
    pub pe_block_size: usize,
    pub encrypted_verifier_hash_input: Vec<u8>,
    pub encrypted_verifier_hash_value: Vec<u8>,
    pub encrypted_key_value: Vec<u8>,
}

// ---------------------------------------------------------------------------
// RC4 CryptoAPI params
// ---------------------------------------------------------------------------

/// Parameters parsed from the EncryptionInfo binary header for RC4 CryptoAPI.
#[derive(Debug, Clone)]
pub(crate) struct Rc4CryptoApiParams {
    pub salt: [u8; 16],
    pub encrypted_verifier: [u8; 16],
    pub verifier_hash_size: u32,
    pub encrypted_verifier_hash: Vec<u8>,
    pub key_size: u32, // bits (40 or 128)
}

// ---------------------------------------------------------------------------
// Standard AES params (also used for NonStandard AES with iterations=0)
// ---------------------------------------------------------------------------

/// Parameters for Standard AES / NonStandard AES encryption.
#[derive(Debug, Clone)]
pub(crate) struct StandardAesParams {
    pub salt: [u8; 16],
    pub encrypted_verifier: [u8; 16],
    pub verifier_hash_size: u32,
    pub encrypted_verifier_hash: Vec<u8>, // 32 bytes for AES
    pub key_size: u32,                    // 128, 192, or 256
    pub hash_iterations: u32,             // 50000 (standard) or 0 (non-standard)
}

// ---------------------------------------------------------------------------
// EncryptionParams enum
// ---------------------------------------------------------------------------

/// Parsed encryption parameters from the EncryptionInfo on page 0.
#[derive(Debug, Clone)]
pub(crate) enum EncryptionParams {
    Agile(AgileParams),
    Rc4CryptoApi(Rc4CryptoApiParams),
    StandardAes(StandardAesParams),
}

/// Supported hash algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    fn from_str(s: &str) -> Result<Self, FileError> {
        match s {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            "SHA384" => Ok(Self::Sha384),
            "SHA512" => Ok(Self::Sha512),
            _ => Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported hash algorithm: {s}"),
            }),
        }
    }

    fn hash_size(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

// ---------------------------------------------------------------------------
// Generic hash helper
// ---------------------------------------------------------------------------

fn hash_with<D: Digest>(data: &[u8]) -> Vec<u8> {
    let mut h = D::new();
    h.update(data);
    h.finalize().to_vec()
}

fn hash_bytes(algo: HashAlgorithm, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgorithm::Sha1 => hash_with::<Sha1>(data),
        HashAlgorithm::Sha256 => hash_with::<Sha256>(data),
        HashAlgorithm::Sha384 => hash_with::<Sha384>(data),
        HashAlgorithm::Sha512 => hash_with::<Sha512>(data),
    }
}

// ---------------------------------------------------------------------------
// XML parsing with quick-xml
// ---------------------------------------------------------------------------

/// Parse an attribute value from a quick-xml `BytesStart` element.
fn get_attr(
    e: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, FileError> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| FileError::UnsupportedEncryption {
            reason: format!("invalid XML attribute: {err}"),
        })?;
        if attr.key.as_ref() == name {
            // The EncryptionInfo XML declares version 1.0, so attribute values
            // are normalized by the XML 1.0 rules.
            let val = attr
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|err| FileError::UnsupportedEncryption {
                    reason: format!("invalid XML attribute value: {err}"),
                })?;
            return Ok(Some(val.into_owned()));
        }
    }
    Ok(None)
}

fn require_attr(
    e: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
    tag_name: &str,
) -> Result<String, FileError> {
    get_attr(e, name)?.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: format!(
            "missing {}.{} in EncryptionInfo",
            tag_name,
            String::from_utf8_lossy(name)
        ),
    })
}

fn parse_usize(val: &str, tag: &str, attr: &str) -> Result<usize, FileError> {
    val.parse().map_err(|_| FileError::UnsupportedEncryption {
        reason: format!("invalid {tag}.{attr} value: {val}"),
    })
}

fn parse_u32(val: &str, tag: &str, attr: &str) -> Result<u32, FileError> {
    val.parse().map_err(|_| FileError::UnsupportedEncryption {
        reason: format!("invalid {tag}.{attr} value: {val}"),
    })
}

fn parse_base64(val: &str, tag: &str, attr: &str) -> Result<Vec<u8>, FileError> {
    BASE64
        .decode(val)
        .map_err(|_| FileError::UnsupportedEncryption {
            reason: format!("invalid base64 in {tag}.{attr}"),
        })
}

/// Data extracted from the `<keyData>` element.
struct KeyDataAttrs {
    key_bits: usize,
    block_size: usize,
    hash_algorithm: HashAlgorithm,
    salt_value: Vec<u8>,
}

/// Data extracted from the `<*:encryptedKey>` element.
struct EncryptedKeyAttrs {
    spin_count: u32,
    salt_value: Vec<u8>,
    hash_algorithm: HashAlgorithm,
    key_bits: usize,
    block_size: usize,
    encrypted_verifier_hash_input: Vec<u8>,
    encrypted_verifier_hash_value: Vec<u8>,
    encrypted_key_value: Vec<u8>,
}

fn parse_key_data(e: &quick_xml::events::BytesStart<'_>) -> Result<KeyDataAttrs, FileError> {
    let tag = "keyData";
    Ok(KeyDataAttrs {
        key_bits: parse_usize(&require_attr(e, b"keyBits", tag)?, tag, "keyBits")?,
        block_size: parse_usize(&require_attr(e, b"blockSize", tag)?, tag, "blockSize")?,
        hash_algorithm: HashAlgorithm::from_str(&require_attr(e, b"hashAlgorithm", tag)?)?,
        salt_value: parse_base64(&require_attr(e, b"saltValue", tag)?, tag, "saltValue")?,
    })
}

fn parse_encrypted_key(
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<EncryptedKeyAttrs, FileError> {
    let tag = "encryptedKey";
    Ok(EncryptedKeyAttrs {
        spin_count: parse_u32(&require_attr(e, b"spinCount", tag)?, tag, "spinCount")?,
        salt_value: parse_base64(&require_attr(e, b"saltValue", tag)?, tag, "saltValue")?,
        hash_algorithm: HashAlgorithm::from_str(&require_attr(e, b"hashAlgorithm", tag)?)?,
        key_bits: parse_usize(&require_attr(e, b"keyBits", tag)?, tag, "keyBits")?,
        block_size: parse_usize(&require_attr(e, b"blockSize", tag)?, tag, "blockSize")?,
        encrypted_verifier_hash_input: parse_base64(
            &require_attr(e, b"encryptedVerifierHashInput", tag)?,
            tag,
            "encryptedVerifierHashInput",
        )?,
        encrypted_verifier_hash_value: parse_base64(
            &require_attr(e, b"encryptedVerifierHashValue", tag)?,
            tag,
            "encryptedVerifierHashValue",
        )?,
        encrypted_key_value: parse_base64(
            &require_attr(e, b"encryptedKeyValue", tag)?,
            tag,
            "encryptedKeyValue",
        )?,
    })
}

// ---------------------------------------------------------------------------
// parse_encryption_info
// ---------------------------------------------------------------------------

/// Parse the EncryptionInfo from page 0.
/// Returns `None` if no encryption is present.
pub(crate) fn parse_encryption_info(page0: &[u8]) -> Result<Option<EncryptionParams>, FileError> {
    let offset = db_header::ENCRYPTION_INFO_OFFSET;
    if page0.len() < offset + 2 {
        return Ok(None);
    }

    // EncryptionInfo length is stored as u16 LE.
    let info_len = u16::from_le_bytes([page0[offset], page0[offset + 1]]) as usize;
    if info_len == 0 {
        return Ok(None);
    }

    let info_start = offset + 2;
    let info_end = info_start + info_len;
    if page0.len() < info_end {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionInfo extends beyond page 0".to_string(),
        });
    }

    let info_data = &page0[info_start..info_end];
    if info_data.len() < 4 {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionInfo too short for version header".to_string(),
        });
    }

    let v_major = u16::from_le_bytes([info_data[0], info_data[1]]);
    let v_minor = u16::from_le_bytes([info_data[2], info_data[3]]);

    match (v_major, v_minor) {
        // Agile Encryption (MS-OFFCRYPTO §2.3.4.10)
        (4, 4) => {
            parse_agile_encryption_info(&info_data[4..]).map(|p| Some(EncryptionParams::Agile(p)))
        }

        // Office Binary Doc RC4 (not supported)
        (1, 1) => Err(FileError::UnsupportedEncryption {
            reason: "Office Binary Doc RC4 encryption (v1.1) is not supported".to_string(),
        }),

        // Extensible Encryption (not supported)
        (3 | 4, 3) => Err(FileError::UnsupportedEncryption {
            reason: "Extensible encryption (v3/4.3) is not supported".to_string(),
        }),

        // RC4 CryptoAPI / Standard AES / NonStandard AES
        (2..=4, 2) => {
            if info_data.len() < 8 {
                return Err(FileError::UnsupportedEncryption {
                    reason: "EncryptionInfo too short for CryptoAPI flags".to_string(),
                });
            }
            let flags =
                u32::from_le_bytes([info_data[4], info_data[5], info_data[6], info_data[7]]);

            if flags & FCRYPTO_API_FLAG == 0 {
                return Err(FileError::UnsupportedEncryption {
                    reason: format!(
                        "CryptoAPI flag not set in EncryptionInfo flags: 0x{flags:08x}"
                    ),
                });
            }

            if flags & FAES_FLAG != 0 {
                // Standard AES Encryption (MS-OFFCRYPTO §2.3.4.5)
                parse_standard_aes_info(&info_data[8..], 50_000)
                    .map(|p| Some(EncryptionParams::StandardAes(p)))
            } else {
                // Try RC4 CryptoAPI first; if algId is AES, fall back to NonStandard AES
                parse_cryptoapi_info(&info_data[8..])
            }
        }

        _ => Err(FileError::UnsupportedEncryption {
            reason: format!(
                "unsupported EncryptionInfo version: vMajor={v_major}, vMinor={v_minor}"
            ),
        }),
    }
}

/// Parse Agile Encryption XML from the EncryptionInfo payload.
fn parse_agile_encryption_info(xml_bytes: &[u8]) -> Result<AgileParams, FileError> {
    let mut reader = quick_xml::Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut key_data: Option<KeyDataAttrs> = None;
    let mut enc_key: Option<EncryptedKeyAttrs> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"keyData" => {
                        key_data = Some(parse_key_data(e)?);
                    }
                    b"encryptedKey" => {
                        enc_key = Some(parse_encrypted_key(e)?);
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(err) => {
                return Err(FileError::UnsupportedEncryption {
                    reason: format!("EncryptionInfo XML parse error: {err}"),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    let kd = key_data.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: "missing <keyData> in EncryptionInfo XML".to_string(),
    })?;
    let ek = enc_key.ok_or_else(|| FileError::UnsupportedEncryption {
        reason: "missing <encryptedKey> in EncryptionInfo XML".to_string(),
    })?;

    Ok(AgileParams {
        key_bits: kd.key_bits,
        block_size: kd.block_size,
        hash_algorithm: kd.hash_algorithm,
        salt_value: kd.salt_value,
        pe_spin_count: ek.spin_count,
        pe_salt_value: ek.salt_value,
        pe_hash_algorithm: ek.hash_algorithm,
        pe_key_bits: ek.key_bits,
        pe_block_size: ek.block_size,
        encrypted_verifier_hash_input: ek.encrypted_verifier_hash_input,
        encrypted_verifier_hash_value: ek.encrypted_verifier_hash_value,
        encrypted_key_value: ek.encrypted_key_value,
    })
}

// ---------------------------------------------------------------------------
// RC4 CryptoAPI / Standard AES binary parsing
// ---------------------------------------------------------------------------

/// Read a u32 LE from data at the given offset.
fn read_u32(data: &[u8], off: usize) -> Result<u32, FileError> {
    if data.len() < off + 4 {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader/Verifier truncated".to_string(),
        });
    }
    Ok(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

/// Parse a CryptoAPI EncryptionHeader + EncryptionVerifier.
/// Dispatches to Rc4CryptoApi or StandardAes(iterations=0) based on algId.
fn parse_cryptoapi_info(data: &[u8]) -> Result<Option<EncryptionParams>, FileError> {
    // headerSize (4 bytes) followed by EncryptionHeader
    let header_size = read_u32(data, 0)? as usize;
    let header_end = 4usize
        .checked_add(header_size)
        .ok_or(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader size overflow".to_string(),
        })?;
    if data.len() < header_end {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader extends beyond EncryptionInfo".to_string(),
        });
    }

    let header_data = &data[4..header_end];

    // EncryptionHeader: flags(4) + sizeExtra(4) + algId(4) + algIdHash(4) + keySize(4) + ...
    if header_data.len() < 20 {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader too small".to_string(),
        });
    }
    let alg_id = read_u32(header_data, 8)?;
    let key_size = read_u32(header_data, 16)?;

    // Verifier data follows the header
    let verifier_data = &data[header_end..];

    match alg_id {
        ALGID_RC4 => {
            let key_size = if key_size == 0 { 40 } else { key_size };
            if key_size != 40 && key_size != 128 {
                return Err(FileError::UnsupportedEncryption {
                    reason: format!("invalid RC4 CryptoAPI key size: {key_size}"),
                });
            }
            let v = parse_encryption_verifier(verifier_data, 20)?;
            Ok(Some(EncryptionParams::Rc4CryptoApi(Rc4CryptoApiParams {
                salt: v.salt,
                encrypted_verifier: v.encrypted_verifier,
                verifier_hash_size: v.verifier_hash_size,
                encrypted_verifier_hash: v.encrypted_verifier_hash,
                key_size,
            })))
        }
        ALGID_AES_128 | ALGID_AES_192 | ALGID_AES_256 => {
            // NonStandard AES: same binary format as Standard AES but iterations=0
            let key_size = match alg_id {
                ALGID_AES_128 => {
                    if key_size == 0 {
                        128
                    } else {
                        key_size
                    }
                }
                ALGID_AES_192 => {
                    if key_size == 0 {
                        192
                    } else {
                        key_size
                    }
                }
                ALGID_AES_256 => {
                    if key_size == 0 {
                        256
                    } else {
                        key_size
                    }
                }
                _ => unreachable!(),
            };
            if key_size != 128 && key_size != 192 && key_size != 256 {
                return Err(FileError::UnsupportedEncryption {
                    reason: format!("invalid AES key size: {key_size}"),
                });
            }
            parse_standard_aes_info_from_verifier(verifier_data, key_size, 0)
                .map(|p| Some(EncryptionParams::StandardAes(p)))
        }
        _ => Err(FileError::UnsupportedEncryption {
            reason: format!("unsupported CryptoAPI algorithm ID: 0x{alg_id:04x}"),
        }),
    }
}

/// Parse a Standard AES EncryptionInfo (with headerSize + EncryptionHeader + EncryptionVerifier).
fn parse_standard_aes_info(
    data: &[u8],
    hash_iterations: u32,
) -> Result<StandardAesParams, FileError> {
    let header_size = read_u32(data, 0)? as usize;
    let header_end = 4usize
        .checked_add(header_size)
        .ok_or(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader size overflow".to_string(),
        })?;
    if data.len() < header_end {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader extends beyond EncryptionInfo".to_string(),
        });
    }

    let header_data = &data[4..header_end];
    if header_data.len() < 20 {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionHeader too small".to_string(),
        });
    }
    let alg_id = read_u32(header_data, 8)?;
    let key_size_raw = read_u32(header_data, 16)?;

    let key_size = match alg_id {
        ALGID_AES_128 => {
            if key_size_raw == 0 {
                128
            } else {
                key_size_raw
            }
        }
        ALGID_AES_192 => {
            if key_size_raw == 0 {
                192
            } else {
                key_size_raw
            }
        }
        ALGID_AES_256 => {
            if key_size_raw == 0 {
                256
            } else {
                key_size_raw
            }
        }
        _ => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("Standard AES: unexpected algId 0x{alg_id:04x}"),
            })
        }
    };

    let verifier_data = &data[header_end..];
    parse_standard_aes_info_from_verifier(verifier_data, key_size, hash_iterations)
}

/// Parsed EncryptionVerifier fields.
struct VerifierFields {
    salt: [u8; 16],
    encrypted_verifier: [u8; 16],
    verifier_hash_size: u32,
    encrypted_verifier_hash: Vec<u8>,
}

/// Parse the EncryptionVerifier from binary data.
fn parse_encryption_verifier(
    data: &[u8],
    enc_verifier_hash_len: usize,
) -> Result<VerifierFields, FileError> {
    // saltSize(4) + salt(16) + encryptedVerifier(16) + verifierHashSize(4) + encryptedVerifierHash(N)
    let min_len = 4 + 16 + 16 + 4 + enc_verifier_hash_len;
    if data.len() < min_len {
        return Err(FileError::UnsupportedEncryption {
            reason: "EncryptionVerifier truncated".to_string(),
        });
    }

    let salt_size = read_u32(data, 0)?;
    if salt_size != 16 {
        return Err(FileError::UnsupportedEncryption {
            reason: format!("unexpected salt size: {salt_size} (expected 16)"),
        });
    }

    let mut salt = [0u8; 16];
    salt.copy_from_slice(&data[4..20]);

    let mut encrypted_verifier = [0u8; 16];
    encrypted_verifier.copy_from_slice(&data[20..36]);

    let verifier_hash_size = read_u32(data, 36)?;

    let encrypted_verifier_hash = data[40..40 + enc_verifier_hash_len].to_vec();

    Ok(VerifierFields {
        salt,
        encrypted_verifier,
        verifier_hash_size,
        encrypted_verifier_hash,
    })
}

/// Parse EncryptionVerifier and build StandardAesParams.
fn parse_standard_aes_info_from_verifier(
    verifier_data: &[u8],
    key_size: u32,
    hash_iterations: u32,
) -> Result<StandardAesParams, FileError> {
    // AES encrypted verifier hash is 32 bytes (padded to AES block size)
    let v = parse_encryption_verifier(verifier_data, 32)?;

    Ok(StandardAesParams {
        salt: v.salt,
        encrypted_verifier: v.encrypted_verifier,
        verifier_hash_size: v.verifier_hash_size,
        encrypted_verifier_hash: v.encrypted_verifier_hash,
        key_size,
        hash_iterations,
    })
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive an encryption key using the Agile Encryption key derivation scheme.
///
/// 1. H0 = hash(salt + password_utf16le)
/// 2. Hn = hash(n_u32le + H_{n-1}) for spinCount iterations
/// 3. Hfinal = hash(H_last + blockKey)
/// 4. Pad/truncate to cbRequiredKeyLength bytes
fn derive_key(
    password: &str,
    salt: &[u8],
    spin_count: u32,
    block_key: &[u8],
    hash_algo: HashAlgorithm,
    key_bits: usize,
) -> Zeroizing<Vec<u8>> {
    // H0 = hash(salt + password_utf16le)
    let password_utf16: Zeroizing<Vec<u8>> = Zeroizing::new(
        password
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect(),
    );

    let mut buf = Vec::with_capacity(salt.len() + password_utf16.len());
    buf.extend_from_slice(salt);
    buf.extend_from_slice(&password_utf16);
    let mut h = Zeroizing::new(hash_bytes(hash_algo, &buf));

    // Hn = hash(n_u32le + H_{n-1})
    for n in 0..spin_count {
        let mut iter_buf = Vec::with_capacity(4 + h.len());
        iter_buf.extend_from_slice(&n.to_le_bytes());
        iter_buf.extend_from_slice(&h);
        h = Zeroizing::new(hash_bytes(hash_algo, &iter_buf));
    }

    // Hfinal = hash(H_last + blockKey)
    let mut final_buf = Vec::with_capacity(h.len() + block_key.len());
    final_buf.extend_from_slice(&h);
    final_buf.extend_from_slice(block_key);
    let h_final = Zeroizing::new(hash_bytes(hash_algo, &final_buf));

    // Pad or truncate to key_bits / 8
    let key_len = key_bits / 8;
    if h_final.len() >= key_len {
        Zeroizing::new(h_final[..key_len].to_vec())
    } else {
        let mut padded = h_final.to_vec();
        padded.resize(key_len, HASH_PAD_BYTE);
        Zeroizing::new(padded)
    }
}

// ---------------------------------------------------------------------------
// AES-CBC decryption helper
// ---------------------------------------------------------------------------

/// Decrypt data using AES-CBC with the given key and IV.
/// Returns the decrypted data (no PKCS7 unpadding — the caller handles truncation).
fn aes_cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, FileError> {
    // AES-CBC requires data to be a multiple of block size (16).
    // Pad input to block boundary if needed.
    let block_size = 16;
    let padded_len = if data.len() % block_size != 0 {
        (data.len() / block_size + 1) * block_size
    } else {
        data.len()
    };
    let mut buf = vec![0u8; padded_len];
    buf[..data.len()].copy_from_slice(data);

    // Ensure IV is exactly 16 bytes
    let mut iv16 = [0u8; 16];
    let copy_len = iv.len().min(16);
    iv16[..copy_len].copy_from_slice(&iv[..copy_len]);

    match key.len() {
        16 => {
            let mut key16 = [0u8; 16];
            key16.copy_from_slice(key);
            Aes128CbcDec::new(&key16.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-CBC decryption failed".to_string(),
                })?;
        }
        24 => {
            let mut key24 = [0u8; 24];
            key24.copy_from_slice(key);
            Aes192CbcDec::new(&key24.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-192-CBC decryption failed".to_string(),
                })?;
        }
        32 => {
            let mut key32 = [0u8; 32];
            key32.copy_from_slice(key);
            Aes256CbcDec::new(&key32.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-CBC decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported AES key size: {other} bytes"),
            });
        }
    }

    Ok(buf)
}

// ---------------------------------------------------------------------------
// verify_password
// ---------------------------------------------------------------------------

/// Verify the password against the EncryptionInfo parameters.
/// On success, returns the decrypted database key (encryptedKeyValue).
pub(crate) fn verify_password(
    params: &AgileParams,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, FileError> {
    let algo = params.pe_hash_algorithm;
    let salt = &params.pe_salt_value;
    let spin = params.pe_spin_count;
    let key_bits = params.pe_key_bits;

    // Derive the three keys
    let key_input = derive_key(
        password,
        salt,
        spin,
        &VERIFIER_HASH_INPUT_BLOCK_KEY,
        algo,
        key_bits,
    );
    let key_hash_value = derive_key(
        password,
        salt,
        spin,
        &VERIFIER_HASH_VALUE_BLOCK_KEY,
        algo,
        key_bits,
    );
    let key_enc_key = derive_key(
        password,
        salt,
        spin,
        &ENCRYPTED_KEY_VALUE_BLOCK_KEY,
        algo,
        key_bits,
    );

    // IV for password encryptor decryption: salt padded/truncated to pe_block_size
    let iv = make_iv(salt, params.pe_block_size);

    // Decrypt verifierHashInput
    let verifier = Zeroizing::new(aes_cbc_decrypt(
        &key_input,
        &iv,
        &params.encrypted_verifier_hash_input,
    )?);

    // Decrypt verifierHashValue
    let expected_hash_full = Zeroizing::new(aes_cbc_decrypt(
        &key_hash_value,
        &iv,
        &params.encrypted_verifier_hash_value,
    )?);

    // Verify: hash(verifier) == expected_hash (truncated to hash size)
    let hash_size = algo.hash_size();
    let actual_hash = Zeroizing::new(hash_bytes(algo, &verifier));

    let expected_hash = if expected_hash_full.len() >= hash_size {
        &expected_hash_full[..hash_size]
    } else {
        &expected_hash_full
    };

    if actual_hash[..hash_size].ct_eq(expected_hash).unwrap_u8() != 1 {
        return Err(FileError::InvalidPassword);
    }

    // Decrypt the database key
    let db_key = Zeroizing::new(aes_cbc_decrypt(
        &key_enc_key,
        &iv,
        &params.encrypted_key_value,
    )?);

    // Truncate to keyData keyBits / 8
    let db_key_len = params.key_bits / 8;
    Ok(Zeroizing::new(db_key[..db_key_len].to_vec()))
}

// ---------------------------------------------------------------------------
// Page decryption
// ---------------------------------------------------------------------------

/// Build an IV by padding/truncating data to the target block size.
fn make_iv(data: &[u8], block_size: usize) -> Vec<u8> {
    if data.len() >= block_size {
        data[..block_size].to_vec()
    } else {
        let mut iv = vec![HASH_PAD_BYTE; block_size];
        iv[..data.len()].copy_from_slice(data);
        iv
    }
}

/// Decrypt a data page using Agile Encryption (Access-specific scheme).
///
/// IV = hash(db_salt + block_key_bytes), padded/truncated to blockSize.
/// block_key = page_number XOR db_encoding_key.
pub(crate) fn decrypt_page_agile(
    buf: &mut [u8],
    params: &AgileParams,
    db_key: &[u8],
    db_encoding_key: u32,
    page: u32,
) -> Result<(), FileError> {
    if buf.is_empty() {
        return Ok(());
    }

    let block_key = page ^ db_encoding_key;
    let block_key_bytes = block_key.to_le_bytes();

    // IV = hash(keyData.salt + block_key_bytes)
    let mut iv_input = Vec::with_capacity(params.salt_value.len() + 4);
    iv_input.extend_from_slice(&params.salt_value);
    iv_input.extend_from_slice(&block_key_bytes);
    let iv_hash = hash_bytes(params.hash_algorithm, &iv_input);
    let iv = make_iv(&iv_hash, params.block_size);

    // Decrypt entire page with AES-CBC
    // Ensure buffer is block-aligned for AES
    let aes_block = 16;
    let decrypt_len = (buf.len() / aes_block) * aes_block;
    if decrypt_len == 0 {
        return Ok(());
    }

    let mut iv16 = [0u8; 16];
    let copy_len = iv.len().min(16);
    iv16[..copy_len].copy_from_slice(&iv[..copy_len]);

    match db_key.len() {
        16 => {
            let mut key16 = [0u8; 16];
            key16.copy_from_slice(db_key);
            Aes128CbcDec::new(&key16.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-CBC page decryption failed".to_string(),
                })?;
        }
        24 => {
            let mut key24 = [0u8; 24];
            key24.copy_from_slice(db_key);
            Aes192CbcDec::new(&key24.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-192-CBC page decryption failed".to_string(),
                })?;
        }
        32 => {
            let mut key32 = [0u8; 32];
            key32.copy_from_slice(db_key);
            Aes256CbcDec::new(&key32.into(), &iv16.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-CBC page decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported db_key length: {other} bytes"),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// RC4 CryptoAPI password verification and page decryption
// ---------------------------------------------------------------------------

/// Compute block_bytes for page decryption (MS-OFFCRYPTO applyPageNumber).
/// XORs the encoding key bytes with the little-endian page number.
/// (jackcessencrypt uses LITTLE_ENDIAN ByteBuffer order)
fn apply_page_number(encoding_key: &[u8; 4], page: u32) -> [u8; 4] {
    let page_le = page.to_le_bytes();
    [
        encoding_key[0] ^ page_le[0],
        encoding_key[1] ^ page_le[1],
        encoding_key[2] ^ page_le[2],
        encoding_key[3] ^ page_le[3],
    ]
}

/// Derive an RC4 CryptoAPI encryption key from base_hash and block_bytes.
fn derive_rc4_cryptoapi_key(base_hash: &[u8], block_bytes: &[u8], key_size: u32) -> Vec<u8> {
    let key_byte_size = (key_size / 8) as usize;
    // enc_key = SHA1(base_hash || block_bytes), truncated to key_byte_size
    let mut input = Vec::with_capacity(base_hash.len() + block_bytes.len());
    input.extend_from_slice(base_hash);
    input.extend_from_slice(block_bytes);
    let hash = hash_bytes(HashAlgorithm::Sha1, &input);

    let mut enc_key = hash[..key_byte_size.min(hash.len())].to_vec();
    // For 40-bit keys, zero-pad to 128 bits (16 bytes)
    if key_size == 40 {
        enc_key.resize(16, 0);
    }
    enc_key
}

/// Verify a password for RC4 CryptoAPI encryption.
/// Returns the base_hash on success (used for page decryption).
pub(crate) fn verify_password_rc4_cryptoapi(
    params: &Rc4CryptoApiParams,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, FileError> {
    // base_hash = SHA1(salt + password_utf16le)
    let password_utf16: Zeroizing<Vec<u8>> = Zeroizing::new(
        password
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect(),
    );

    let mut salt_pw = Vec::with_capacity(16 + password_utf16.len());
    salt_pw.extend_from_slice(&params.salt);
    salt_pw.extend_from_slice(&password_utf16);
    let base_hash = Zeroizing::new(hash_bytes(HashAlgorithm::Sha1, &salt_pw));

    // Verification uses block_bytes = [0, 0, 0, 0]
    let enc_key = derive_rc4_cryptoapi_key(&base_hash, &[0u8; 4], params.key_size);

    // Decrypt verifier and verifier_hash with the same RC4 stream
    let mut combined = Vec::with_capacity(16 + params.encrypted_verifier_hash.len());
    combined.extend_from_slice(&params.encrypted_verifier);
    combined.extend_from_slice(&params.encrypted_verifier_hash);
    rc4_transform(&enc_key, &mut combined);

    let verifier = &combined[..16];
    let verifier_hash = &combined[16..];

    // Compute SHA1(verifier) and compare with decrypted hash
    let actual_hash = hash_bytes(HashAlgorithm::Sha1, verifier);
    let hash_size = params.verifier_hash_size as usize;
    if hash_size == 0 || hash_size > 20 {
        return Err(FileError::UnsupportedEncryption {
            reason: format!("invalid verifier hash size: {hash_size}"),
        });
    }
    let expected = &verifier_hash[..hash_size.min(verifier_hash.len())];
    let actual = &actual_hash[..hash_size.min(actual_hash.len())];

    if actual.ct_eq(expected).unwrap_u8() != 1 {
        return Err(FileError::InvalidPassword);
    }

    Ok(base_hash)
}

/// Decrypt a data page using RC4 CryptoAPI encryption.
pub(crate) fn decrypt_page_rc4_cryptoapi(
    buf: &mut [u8],
    base_hash: &[u8],
    encoding_key: &[u8; 4],
    key_size: u32,
    page: u32,
) {
    if buf.is_empty() {
        return;
    }
    let block_bytes = apply_page_number(encoding_key, page);
    let enc_key = derive_rc4_cryptoapi_key(base_hash, &block_bytes, key_size);
    rc4_transform(&enc_key, buf);
}

// ---------------------------------------------------------------------------
// Standard AES / NonStandard AES password verification and page decryption
// ---------------------------------------------------------------------------

/// Iterate hash: H_n = SHA1(n_u32le + H_{n-1}) for n in 0..iterations.
/// If iterations == 0, returns base_hash unchanged.
fn iterate_hash_sha1(base_hash: &[u8], iterations: u32) -> Vec<u8> {
    if iterations == 0 {
        return base_hash.to_vec();
    }

    let mut h = Zeroizing::new(base_hash.to_vec());
    for i in 0..iterations {
        let mut buf = Vec::with_capacity(4 + h.len());
        buf.extend_from_slice(&i.to_le_bytes());
        buf.extend_from_slice(&h);
        h = Zeroizing::new(hash_bytes(HashAlgorithm::Sha1, &buf));
    }
    h.to_vec()
}

/// Generate XOR-padded bytes for HMAC-like key derivation (MS-OFFCRYPTO §2.3.4.7).
fn gen_x_bytes(final_hash: &[u8], code: u8) -> [u8; 64] {
    let mut x = [code; 64];
    for (i, &b) in final_hash.iter().enumerate() {
        x[i] ^= b;
    }
    x
}

/// Derive a Standard AES encryption key using cryptDeriveKey (MS-OFFCRYPTO §2.3.4.7).
fn derive_standard_aes_key(iter_hash: &[u8], block_bytes: &[u8], key_byte_len: usize) -> Vec<u8> {
    // final_hash = SHA1(iter_hash || block_bytes)
    let mut buf = Vec::with_capacity(iter_hash.len() + block_bytes.len());
    buf.extend_from_slice(iter_hash);
    buf.extend_from_slice(block_bytes);
    let final_hash = Zeroizing::new(hash_bytes(HashAlgorithm::Sha1, &buf));

    // x1 = SHA1(genXBytes(final_hash, 0x36))
    let x1 = Zeroizing::new(hash_bytes(
        HashAlgorithm::Sha1,
        &gen_x_bytes(&final_hash, 0x36),
    ));
    // x2 = SHA1(genXBytes(final_hash, 0x5C))
    let x2 = Zeroizing::new(hash_bytes(
        HashAlgorithm::Sha1,
        &gen_x_bytes(&final_hash, 0x5C),
    ));

    // enc_key = (x1 || x2)[..key_byte_len]
    let mut full = Vec::with_capacity(x1.len() + x2.len());
    full.extend_from_slice(&x1);
    full.extend_from_slice(&x2);
    full.truncate(key_byte_len);
    // Zero-pad if needed (shouldn't happen for AES-128/192/256 with SHA1)
    full.resize(key_byte_len, 0);
    full
}

/// Decrypt data using AES-ECB (used for Standard AES / NonStandard AES).
fn aes_ecb_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>, FileError> {
    let block_size = 16;
    let padded_len = if data.len() % block_size != 0 {
        (data.len() / block_size + 1) * block_size
    } else {
        data.len()
    };
    let mut buf = vec![0u8; padded_len];
    buf[..data.len()].copy_from_slice(data);

    match key.len() {
        16 => {
            Aes128EcbDec::new(key.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-ECB decryption failed".to_string(),
                })?;
        }
        24 => {
            Aes192EcbDec::new(key.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-192-ECB decryption failed".to_string(),
                })?;
        }
        32 => {
            Aes256EcbDec::new(key.into())
                .decrypt_padded_mut::<NoPadding>(&mut buf)
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-ECB decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported AES key size: {other} bytes"),
            });
        }
    }

    Ok(buf)
}

/// Verify a password for Standard AES / NonStandard AES encryption.
/// Returns the iter_hash on success (precomputed for page decryption).
pub(crate) fn verify_password_standard_aes(
    params: &StandardAesParams,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, FileError> {
    // base_hash = SHA1(salt + password_utf16le)
    let password_utf16: Zeroizing<Vec<u8>> = Zeroizing::new(
        password
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect(),
    );

    let mut salt_pw = Vec::with_capacity(16 + password_utf16.len());
    salt_pw.extend_from_slice(&params.salt);
    salt_pw.extend_from_slice(&password_utf16);
    let base_hash = Zeroizing::new(hash_bytes(HashAlgorithm::Sha1, &salt_pw));

    // Iterate hash
    let iter_hash = Zeroizing::new(iterate_hash_sha1(&base_hash, params.hash_iterations));

    // Verification uses block_bytes = [0, 0, 0, 0]
    let key_byte_len = (params.key_size / 8) as usize;
    let enc_key = derive_standard_aes_key(&iter_hash, &[0u8; 4], key_byte_len);

    // Decrypt verifier and verifier_hash with AES-ECB
    let verifier = aes_ecb_decrypt(&enc_key, &params.encrypted_verifier)?;
    let verifier_hash_dec = aes_ecb_decrypt(&enc_key, &params.encrypted_verifier_hash)?;

    // SHA1(verifier) == verifier_hash (truncated to verifier_hash_size)
    let actual_hash = hash_bytes(HashAlgorithm::Sha1, &verifier);
    let hash_size = params.verifier_hash_size as usize;
    if hash_size == 0 || hash_size > 20 {
        return Err(FileError::UnsupportedEncryption {
            reason: format!("invalid verifier hash size: {hash_size}"),
        });
    }
    let expected = &verifier_hash_dec[..hash_size.min(verifier_hash_dec.len())];
    let actual = &actual_hash[..hash_size.min(actual_hash.len())];

    if actual.ct_eq(expected).unwrap_u8() != 1 {
        return Err(FileError::InvalidPassword);
    }

    Ok(iter_hash)
}

/// Decrypt a data page using Standard AES / NonStandard AES (AES-ECB).
pub(crate) fn decrypt_page_standard_aes(
    buf: &mut [u8],
    iter_hash: &[u8],
    encoding_key: &[u8; 4],
    key_size: u32,
    page: u32,
) -> Result<(), FileError> {
    if buf.is_empty() {
        return Ok(());
    }

    let block_bytes = apply_page_number(encoding_key, page);
    let key_byte_len = (key_size / 8) as usize;
    let enc_key = derive_standard_aes_key(iter_hash, &block_bytes, key_byte_len);

    // Decrypt in-place using AES-ECB
    let aes_block = 16;
    let decrypt_len = (buf.len() / aes_block) * aes_block;
    if decrypt_len == 0 {
        return Ok(());
    }

    match enc_key.len() {
        16 => {
            Aes128EcbDec::new(enc_key[..].into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-128-ECB page decryption failed".to_string(),
                })?;
        }
        24 => {
            Aes192EcbDec::new(enc_key[..].into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-192-ECB page decryption failed".to_string(),
                })?;
        }
        32 => {
            Aes256EcbDec::new(enc_key[..].into())
                .decrypt_padded_mut::<NoPadding>(&mut buf[..decrypt_len])
                .map_err(|_| FileError::UnsupportedEncryption {
                    reason: "AES-256-ECB page decryption failed".to_string(),
                })?;
        }
        other => {
            return Err(FileError::UnsupportedEncryption {
                reason: format!("unsupported AES key size for page decryption: {other} bytes"),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{rc4_transform, HEADER_RC4_KEY};
    use crate::format::JetVersion;

    /// Helper: resolve a test data path, returning None if the file doesn't exist.
    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data_path($path) {
                Some(p) => p,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    fn hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hash_algorithm_from_str_valid() {
        assert_eq!(
            HashAlgorithm::from_str("SHA1").unwrap(),
            HashAlgorithm::Sha1
        );
        assert_eq!(
            HashAlgorithm::from_str("SHA256").unwrap(),
            HashAlgorithm::Sha256
        );
        assert_eq!(
            HashAlgorithm::from_str("SHA384").unwrap(),
            HashAlgorithm::Sha384
        );
        assert_eq!(
            HashAlgorithm::from_str("SHA512").unwrap(),
            HashAlgorithm::Sha512
        );
    }

    #[test]
    fn hash_algorithm_from_str_invalid() {
        assert!(HashAlgorithm::from_str("MD5").is_err());
    }

    #[test]
    fn hash_algorithm_hash_size() {
        assert_eq!(HashAlgorithm::Sha1.hash_size(), 20);
        assert_eq!(HashAlgorithm::Sha256.hash_size(), 32);
        assert_eq!(HashAlgorithm::Sha384.hash_size(), 48);
        assert_eq!(HashAlgorithm::Sha512.hash_size(), 64);
    }

    #[test]
    fn hash_bytes_sha256() {
        let data = b"hello";
        let result = hash_bytes(HashAlgorithm::Sha256, data);
        assert_eq!(result.len(), 32);
        // Known SHA-256 of "hello"
        assert_eq!(
            hex(&result),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn hash_bytes_sha1() {
        let data = b"hello";
        let result = hash_bytes(HashAlgorithm::Sha1, data);
        assert_eq!(result.len(), 20);
        assert_eq!(hex(&result), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    #[test]
    fn derive_key_length() {
        let key = derive_key(
            "test",
            &[0u8; 16],
            100,
            &[0u8; 8],
            HashAlgorithm::Sha256,
            128,
        );
        assert_eq!(key.len(), 16); // 128 bits / 8
    }

    #[test]
    fn derive_key_length_256() {
        let key = derive_key(
            "test",
            &[0u8; 16],
            100,
            &[0u8; 8],
            HashAlgorithm::Sha256,
            256,
        );
        assert_eq!(key.len(), 32); // 256 bits / 8
    }

    #[test]
    fn derive_key_deterministic() {
        // Same inputs must produce the same output
        let k1 = derive_key(
            "pw",
            &[1, 2, 3, 4],
            10,
            &[0xAA; 8],
            HashAlgorithm::Sha256,
            128,
        );
        let k2 = derive_key(
            "pw",
            &[1, 2, 3, 4],
            10,
            &[0xAA; 8],
            HashAlgorithm::Sha256,
            128,
        );
        assert_eq!(*k1, *k2);

        // Different block key → different output
        let k3 = derive_key(
            "pw",
            &[1, 2, 3, 4],
            10,
            &[0xBB; 8],
            HashAlgorithm::Sha256,
            128,
        );
        assert_ne!(*k1, *k3);
    }

    #[test]
    fn make_iv_exact() {
        let data = vec![1u8; 16];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
        assert_eq!(iv, data);
    }

    #[test]
    fn make_iv_truncate() {
        let data = vec![1u8; 32];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
    }

    #[test]
    fn make_iv_pad() {
        let data = vec![1u8; 8];
        let iv = make_iv(&data, 16);
        assert_eq!(iv.len(), 16);
        assert_eq!(&iv[..8], &[1u8; 8]);
        assert_eq!(&iv[8..], &[HASH_PAD_BYTE; 8]);
    }

    #[test]
    fn aes_cbc_decrypt_roundtrip() {
        // Encrypt with known key/iv, then decrypt and verify
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

        let key = [0x42u8; 16];
        let iv = [0u8; 16];
        let plaintext = [0xAAu8; 32]; // 2 AES blocks

        let mut buf = plaintext.to_vec();
        Aes128CbcEnc::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_cbc_decrypt(&key, &iv, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn parse_encryption_info_no_data() {
        let page0 = vec![0u8; 4096];
        let result = parse_encryption_info(&page0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_encryption_info_too_small() {
        let page0 = vec![0u8; 0x299];
        let result = parse_encryption_info(&page0).unwrap();
        assert!(result.is_none());
    }

    /// Test with real .accdb file if available
    #[test]
    fn parse_and_verify_enc_v2007() {
        let path = skip_if_missing!("enc_vbaV2007.accdb");

        // Read page 0 manually
        use std::io::Read;
        let mut file = std::fs::File::open(&path).unwrap();
        let mut page0 = vec![0u8; 4096];
        file.read_exact(&mut page0).unwrap();

        // Decrypt header region (RC4)
        let version = JetVersion::from_byte(page0[db_header::VERSION]).unwrap();
        assert!(version.is_accdb());

        // RC4 decrypt header
        let enc_len = 128;
        let end = db_header::ENCRYPTED_START + enc_len;
        rc4_transform(&HEADER_RC4_KEY, &mut page0[db_header::ENCRYPTED_START..end]);

        // Parse EncryptionInfo
        let enc_params = parse_encryption_info(&page0).unwrap();
        assert!(enc_params.is_some(), "EncryptionInfo should be present");
        let enc_params = enc_params.unwrap();

        // Should be Agile encryption
        let agile_params = match &enc_params {
            EncryptionParams::Agile(p) => p,
            other => panic!("expected Agile, got {other:?}"),
        };

        // Verify correct password
        let db_key = verify_password(agile_params, "1234567890");
        assert!(
            db_key.is_ok(),
            "correct password should verify: {:?}",
            db_key.err()
        );
        let db_key = db_key.unwrap();
        assert_eq!(db_key.len(), agile_params.key_bits / 8);

        // Verify incorrect password
        let result = verify_password(agile_params, "wrongpassword");
        assert!(matches!(result, Err(FileError::InvalidPassword)));
    }

    /// Helper: read page 0 and decrypt the header region for EncryptionInfo parsing.
    fn read_and_decrypt_page0(path: &std::path::Path) -> Vec<u8> {
        use std::io::Read;
        let mut file = std::fs::File::open(path).unwrap();
        let mut page0 = vec![0u8; 4096];
        file.read_exact(&mut page0).unwrap();
        let enc_len = 128;
        let end = db_header::ENCRYPTED_START + enc_len;
        rc4_transform(&HEADER_RC4_KEY, &mut page0[db_header::ENCRYPTED_START..end]);
        page0
    }

    #[test]
    fn parse_and_verify_rc4_cryptoapi() {
        let path = skip_if_missing!("db2007-rc4cryptoapi.accdb");
        let page0 = read_and_decrypt_page0(&path);

        let enc_params = parse_encryption_info(&page0).unwrap();
        assert!(enc_params.is_some(), "EncryptionInfo should be present");
        let enc_params = enc_params.unwrap();

        let params = match &enc_params {
            EncryptionParams::Rc4CryptoApi(p) => p,
            other => panic!("expected Rc4CryptoApi, got {other:?}"),
        };

        assert!(params.key_size == 40 || params.key_size == 128);
        assert!(params.verifier_hash_size > 0 && params.verifier_hash_size <= 20);

        // Correct password
        let base_hash = verify_password_rc4_cryptoapi(params, "Test123");
        assert!(
            base_hash.is_ok(),
            "correct password should verify: {:?}",
            base_hash.err()
        );

        // Wrong password
        let result = verify_password_rc4_cryptoapi(params, "WrongPassword");
        assert!(matches!(result, Err(FileError::InvalidPassword)));
    }

    #[test]
    fn parse_and_verify_nonstandard_aes() {
        let path = skip_if_missing!("db-nonstandard-aes.accdb");
        let page0 = read_and_decrypt_page0(&path);

        let enc_params = parse_encryption_info(&page0).unwrap();
        assert!(enc_params.is_some(), "EncryptionInfo should be present");
        let enc_params = enc_params.unwrap();

        let params = match &enc_params {
            EncryptionParams::StandardAes(p) => p,
            other => panic!("expected StandardAes, got {other:?}"),
        };

        assert!(params.key_size == 128 || params.key_size == 192 || params.key_size == 256);
        assert_eq!(
            params.hash_iterations, 0,
            "NonStandard AES has 0 iterations"
        );
        assert!(params.verifier_hash_size > 0 && params.verifier_hash_size <= 20);

        // Correct password
        let iter_hash = verify_password_standard_aes(params, "password");
        assert!(
            iter_hash.is_ok(),
            "correct password should verify: {:?}",
            iter_hash.err()
        );

        // Wrong password
        let result = verify_password_standard_aes(params, "WrongPassword");
        assert!(matches!(result, Err(FileError::InvalidPassword)));
    }

    // NOTE: No Standard AES test (hash_iterations > 0) because we lack test data —
    // Access/Office-encrypted files with iterations>0 are not publicly available.
    // The NonStandard AES test above covers the shared code path; the only
    // difference is iterate_hash_sha1 looping (vs. returning base_hash unchanged).

    // -- Validation tests (synthetic data) ------------------------------------

    /// Build a minimal page0 with an EncryptionInfo payload at the correct offset.
    fn build_page0_with_info(info_data: &[u8]) -> Vec<u8> {
        let offset = db_header::ENCRYPTION_INFO_OFFSET;
        let total = offset + 2 + info_data.len();
        let mut page0 = vec![0u8; total.max(4096)];
        let len = info_data.len() as u16;
        page0[offset] = len as u8;
        page0[offset + 1] = (len >> 8) as u8;
        page0[offset + 2..offset + 2 + info_data.len()].copy_from_slice(info_data);
        page0
    }

    #[test]
    fn parse_encryption_info_too_short_for_version() {
        // info_data is non-zero length but < 4 bytes: should error
        let page0 = build_page0_with_info(&[0x01, 0x02]);
        let result = parse_encryption_info(&page0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, FileError::UnsupportedEncryption { reason } if reason.contains("too short")),
            "expected 'too short' error, got: {err}"
        );
    }

    #[test]
    fn parse_cryptoapi_header_size_huge() {
        // version (3, 2) + flags with FCRYPTO_API_FLAG but no FAES_FLAG
        // header_size = u32::MAX: on 64-bit this exceeds data length,
        // on 32-bit it triggers checked_add overflow. Either way, must error.
        let mut info_data = vec![0u8; 16];
        info_data[0] = 3; // vMajor=3
        info_data[2] = 2; // vMinor=2
        info_data[4] = 0x04; // FCRYPTO_API_FLAG
                             // header_size = u32::MAX
        info_data[8] = 0xFF;
        info_data[9] = 0xFF;
        info_data[10] = 0xFF;
        info_data[11] = 0xFF;
        let page0 = build_page0_with_info(&info_data);
        let result = parse_encryption_info(&page0);
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { .. })),
            "expected UnsupportedEncryption error, got: {result:?}"
        );
    }

    #[test]
    fn parse_standard_aes_header_size_huge() {
        // version (3, 2) + flags with FCRYPTO_API_FLAG + FAES_FLAG
        // header_size = u32::MAX
        let mut info_data = vec![0u8; 16];
        info_data[0] = 3; // vMajor=3
        info_data[2] = 2; // vMinor=2
        info_data[4] = 0x24; // FCRYPTO_API_FLAG | FAES_FLAG
                             // header_size = u32::MAX
        info_data[8] = 0xFF;
        info_data[9] = 0xFF;
        info_data[10] = 0xFF;
        info_data[11] = 0xFF;
        let page0 = build_page0_with_info(&info_data);
        let result = parse_encryption_info(&page0);
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { .. })),
            "expected UnsupportedEncryption error, got: {result:?}"
        );
    }

    #[test]
    fn parse_cryptoapi_invalid_rc4_key_size() {
        // Build a minimal RC4 CryptoAPI header with invalid key_size=64
        let mut info_data = vec![0u8; 200];
        info_data[0] = 3; // vMajor=3
        info_data[2] = 2; // vMinor=2
        info_data[4] = 0x04; // FCRYPTO_API_FLAG

        // After flags (offset 8 in info_data), CryptoAPI data begins:
        // header_size = 40 (enough for EncryptionHeader)
        info_data[8] = 40;
        // EncryptionHeader at offset 12: flags(4) + sizeExtra(4) + algId(4) + algIdHash(4) + keySize(4)
        // algId = ALGID_RC4 (0x6801) at header offset 8 (info offset 20)
        info_data[20] = 0x01;
        info_data[21] = 0x68;
        // keySize = 64 (invalid for RC4) at header offset 16 (info offset 28)
        info_data[28] = 64;

        let page0 = build_page0_with_info(&info_data);
        let result = parse_encryption_info(&page0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, FileError::UnsupportedEncryption { reason } if reason.contains("invalid RC4")),
            "expected invalid RC4 key size error, got: {err}"
        );
    }

    #[test]
    fn parse_cryptoapi_invalid_aes_key_size() {
        // Build a minimal NonStandard AES header with invalid key_size=64
        let mut info_data = vec![0u8; 200];
        info_data[0] = 3; // vMajor=3
        info_data[2] = 2; // vMinor=2
        info_data[4] = 0x04; // FCRYPTO_API_FLAG (no FAES_FLAG)

        // header_size = 40
        info_data[8] = 40;
        // algId = ALGID_AES_128 (0x660E) at header offset 8 (info offset 20)
        info_data[20] = 0x0E;
        info_data[21] = 0x66;
        // keySize = 64 (invalid for AES) at header offset 16 (info offset 28)
        info_data[28] = 64;

        let page0 = build_page0_with_info(&info_data);
        let result = parse_encryption_info(&page0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, FileError::UnsupportedEncryption { reason } if reason.contains("invalid AES")),
            "expected invalid AES key size error, got: {err}"
        );
    }

    #[test]
    fn verify_rc4_cryptoapi_invalid_hash_size_zero() {
        let params = Rc4CryptoApiParams {
            salt: [0u8; 16],
            encrypted_verifier: [0u8; 16],
            verifier_hash_size: 0,
            encrypted_verifier_hash: vec![0u8; 20],
            key_size: 128,
        };
        let result = verify_password_rc4_cryptoapi(&params, "test");
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { reason }) if reason.contains("invalid verifier hash size")),
            "expected invalid verifier hash size error, got: {result:?}"
        );
    }

    #[test]
    fn verify_rc4_cryptoapi_invalid_hash_size_too_large() {
        let params = Rc4CryptoApiParams {
            salt: [0u8; 16],
            encrypted_verifier: [0u8; 16],
            verifier_hash_size: 21,
            encrypted_verifier_hash: vec![0u8; 21],
            key_size: 128,
        };
        let result = verify_password_rc4_cryptoapi(&params, "test");
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { reason }) if reason.contains("invalid verifier hash size")),
            "expected invalid verifier hash size error, got: {result:?}"
        );
    }

    #[test]
    fn verify_standard_aes_invalid_hash_size_zero() {
        let params = StandardAesParams {
            salt: [0u8; 16],
            encrypted_verifier: [0u8; 16],
            verifier_hash_size: 0,
            encrypted_verifier_hash: vec![0u8; 32],
            key_size: 128,
            hash_iterations: 0,
        };
        let result = verify_password_standard_aes(&params, "test");
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { reason }) if reason.contains("invalid verifier hash size")),
            "expected invalid verifier hash size error, got: {result:?}"
        );
    }

    #[test]
    fn verify_standard_aes_invalid_hash_size_too_large() {
        let params = StandardAesParams {
            salt: [0u8; 16],
            encrypted_verifier: [0u8; 16],
            verifier_hash_size: 21,
            encrypted_verifier_hash: vec![0u8; 32],
            key_size: 128,
            hash_iterations: 0,
        };
        let result = verify_password_standard_aes(&params, "test");
        assert!(
            matches!(&result, Err(FileError::UnsupportedEncryption { reason }) if reason.contains("invalid verifier hash size")),
            "expected invalid verifier hash size error, got: {result:?}"
        );
    }

    // -- AES key size coverage (synthetic roundtrip tests) ---------------------

    #[test]
    fn aes_cbc_decrypt_192_roundtrip() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes192CbcEnc = cbc::Encryptor<aes::Aes192>;

        let key = [0x42u8; 24];
        let iv = [0u8; 16];
        let plaintext = [0xBBu8; 32];

        let mut buf = plaintext.to_vec();
        Aes192CbcEnc::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_cbc_decrypt(&key, &iv, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn aes_cbc_decrypt_256_roundtrip() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

        let key = [0x42u8; 32];
        let iv = [0u8; 16];
        let plaintext = [0xCCu8; 32];

        let mut buf = plaintext.to_vec();
        Aes256CbcEnc::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_cbc_decrypt(&key, &iv, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn aes_ecb_decrypt_128_roundtrip() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes128EcbEnc = ecb::Encryptor<aes::Aes128>;

        let key = [0x11u8; 16];
        let plaintext = [0xAAu8; 32];

        let mut buf = plaintext.to_vec();
        Aes128EcbEnc::new(key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_ecb_decrypt(&key, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn aes_ecb_decrypt_192_roundtrip() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes192EcbEnc = ecb::Encryptor<aes::Aes192>;

        let key = [0x22u8; 24];
        let plaintext = [0xBBu8; 32];

        let mut buf = plaintext.to_vec();
        Aes192EcbEnc::new(key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_ecb_decrypt(&key, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    #[test]
    fn aes_ecb_decrypt_256_roundtrip() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes256EcbEnc = ecb::Encryptor<aes::Aes256>;

        let key = [0x33u8; 32];
        let plaintext = [0xCCu8; 32];

        let mut buf = plaintext.to_vec();
        Aes256EcbEnc::new(key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, 32)
            .unwrap();

        let decrypted = aes_ecb_decrypt(&key, &buf).unwrap();
        assert_eq!(&decrypted[..32], &plaintext);
    }

    // -- Page decryption with different key sizes (synthetic) ------------------

    #[test]
    fn decrypt_page_agile_192() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes192CbcEnc = cbc::Encryptor<aes::Aes192>;

        let db_key = vec![0x55u8; 24]; // AES-192
        let salt = vec![0xAAu8; 16];
        let params = AgileParams {
            key_bits: 192,
            block_size: 16,
            hash_algorithm: HashAlgorithm::Sha256,
            salt_value: salt.clone(),
            pe_spin_count: 0,
            pe_salt_value: vec![0u8; 16],
            pe_hash_algorithm: HashAlgorithm::Sha256,
            pe_key_bits: 192,
            pe_block_size: 16,
            encrypted_verifier_hash_input: vec![0u8; 16],
            encrypted_verifier_hash_value: vec![0u8; 32],
            encrypted_key_value: vec![0u8; 32],
        };
        let encoding_key: u32 = 0x12345678;
        let page: u32 = 1;

        // Compute the IV that decrypt_page_agile will use
        let block_key = page ^ encoding_key;
        let mut iv_input = Vec::new();
        iv_input.extend_from_slice(&salt);
        iv_input.extend_from_slice(&block_key.to_le_bytes());
        let iv_hash = hash_bytes(HashAlgorithm::Sha256, &iv_input);
        let iv = make_iv(&iv_hash, 16);

        // Encrypt known plaintext with the same key/IV
        let plaintext = vec![0xDDu8; 32];
        let mut encrypted = plaintext.clone();
        let mut iv16 = [0u8; 16];
        iv16.copy_from_slice(&iv[..16]);
        let mut key24 = [0u8; 24];
        key24.copy_from_slice(&db_key);
        Aes192CbcEnc::new(&key24.into(), &iv16.into())
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, 32)
            .unwrap();

        // Decrypt and verify
        decrypt_page_agile(&mut encrypted, &params, &db_key, encoding_key, page).unwrap();
        assert_eq!(&encrypted, &plaintext);
    }

    #[test]
    fn decrypt_page_agile_256() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

        let db_key = vec![0x66u8; 32]; // AES-256
        let salt = vec![0xBBu8; 16];
        let params = AgileParams {
            key_bits: 256,
            block_size: 16,
            hash_algorithm: HashAlgorithm::Sha256,
            salt_value: salt.clone(),
            pe_spin_count: 0,
            pe_salt_value: vec![0u8; 16],
            pe_hash_algorithm: HashAlgorithm::Sha256,
            pe_key_bits: 256,
            pe_block_size: 16,
            encrypted_verifier_hash_input: vec![0u8; 16],
            encrypted_verifier_hash_value: vec![0u8; 32],
            encrypted_key_value: vec![0u8; 32],
        };
        let encoding_key: u32 = 0xDEADBEEF;
        let page: u32 = 2;

        let block_key = page ^ encoding_key;
        let mut iv_input = Vec::new();
        iv_input.extend_from_slice(&salt);
        iv_input.extend_from_slice(&block_key.to_le_bytes());
        let iv_hash = hash_bytes(HashAlgorithm::Sha256, &iv_input);
        let iv = make_iv(&iv_hash, 16);

        let plaintext = vec![0xEEu8; 32];
        let mut encrypted = plaintext.clone();
        let mut iv16 = [0u8; 16];
        iv16.copy_from_slice(&iv[..16]);
        let mut key32 = [0u8; 32];
        key32.copy_from_slice(&db_key);
        Aes256CbcEnc::new(&key32.into(), &iv16.into())
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, 32)
            .unwrap();

        decrypt_page_agile(&mut encrypted, &params, &db_key, encoding_key, page).unwrap();
        assert_eq!(&encrypted, &plaintext);
    }

    #[test]
    fn decrypt_page_standard_aes_128() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes128EcbEnc = ecb::Encryptor<aes::Aes128>;

        let iter_hash = hash_bytes(HashAlgorithm::Sha1, b"test_key_material");
        let encoding_key = [0x11u8; 4];
        let page: u32 = 1;
        let key_size: u32 = 128;

        // Derive the key that decrypt_page_standard_aes will use
        let block_bytes = apply_page_number(&encoding_key, page);
        let enc_key = derive_standard_aes_key(&iter_hash, &block_bytes, 16);

        let plaintext = vec![0xAAu8; 32];
        let mut encrypted = plaintext.clone();
        Aes128EcbEnc::new(enc_key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, 32)
            .unwrap();

        decrypt_page_standard_aes(&mut encrypted, &iter_hash, &encoding_key, key_size, page)
            .unwrap();
        assert_eq!(&encrypted, &plaintext);
    }

    #[test]
    fn decrypt_page_standard_aes_192() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes192EcbEnc = ecb::Encryptor<aes::Aes192>;

        let iter_hash = hash_bytes(HashAlgorithm::Sha1, b"test_key_192");
        let encoding_key = [0x22u8; 4];
        let page: u32 = 3;
        let key_size: u32 = 192;

        let block_bytes = apply_page_number(&encoding_key, page);
        let enc_key = derive_standard_aes_key(&iter_hash, &block_bytes, 24);

        let plaintext = vec![0xBBu8; 32];
        let mut encrypted = plaintext.clone();
        Aes192EcbEnc::new(enc_key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, 32)
            .unwrap();

        decrypt_page_standard_aes(&mut encrypted, &iter_hash, &encoding_key, key_size, page)
            .unwrap();
        assert_eq!(&encrypted, &plaintext);
    }

    #[test]
    fn decrypt_page_standard_aes_256() {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyInit};
        type Aes256EcbEnc = ecb::Encryptor<aes::Aes256>;

        let iter_hash = hash_bytes(HashAlgorithm::Sha1, b"test_key_256");
        let encoding_key = [0x33u8; 4];
        let page: u32 = 5;
        let key_size: u32 = 256;

        let block_bytes = apply_page_number(&encoding_key, page);
        let enc_key = derive_standard_aes_key(&iter_hash, &block_bytes, 32);

        let plaintext = vec![0xCCu8; 32];
        let mut encrypted = plaintext.clone();
        Aes256EcbEnc::new(enc_key[..].into())
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, 32)
            .unwrap();

        decrypt_page_standard_aes(&mut encrypted, &iter_hash, &encoding_key, key_size, page)
            .unwrap();
        assert_eq!(&encrypted, &plaintext);
    }

    // -- Hash algorithm coverage (SHA384, SHA512) -----------------------------

    #[test]
    fn hash_bytes_sha384() {
        let result = hash_bytes(HashAlgorithm::Sha384, b"hello");
        assert_eq!(result.len(), 48);
        assert_eq!(
            hex(&result),
            "59e1748777448c69de6b800d7a33bbfb9ff1b463e44354c3553bcdb9c666fa90125a3c79f90397bdf5f6a13de828684f"
        );
    }

    #[test]
    fn hash_bytes_sha512() {
        let result = hash_bytes(HashAlgorithm::Sha512, b"hello");
        assert_eq!(result.len(), 64);
        assert_eq!(
            hex(&result),
            "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043"
        );
    }

    // -- iterate_hash_sha1 with iterations > 0 --------------------------------

    #[test]
    fn iterate_hash_sha1_with_iterations() {
        let base = hash_bytes(HashAlgorithm::Sha1, b"test");
        let result_0 = iterate_hash_sha1(&base, 0);
        assert_eq!(
            result_0, base,
            "0 iterations should return base_hash unchanged"
        );

        let result_1 = iterate_hash_sha1(&base, 1);
        assert_ne!(result_1, base, "1 iteration should differ from base_hash");
        assert_eq!(result_1.len(), 20, "SHA1 output is always 20 bytes");

        // Deterministic
        let result_1b = iterate_hash_sha1(&base, 1);
        assert_eq!(result_1, result_1b);

        // More iterations produce different results
        let result_10 = iterate_hash_sha1(&base, 10);
        assert_ne!(result_10, result_1);
    }

    // -- derive_key with SHA384/SHA512 ----------------------------------------

    #[test]
    fn derive_key_sha384() {
        let key = derive_key("pw", &[0u8; 16], 10, &[0xAA; 8], HashAlgorithm::Sha384, 192);
        assert_eq!(key.len(), 24); // 192 bits / 8
    }

    #[test]
    fn derive_key_sha512() {
        let key = derive_key("pw", &[0u8; 16], 10, &[0xAA; 8], HashAlgorithm::Sha512, 256);
        assert_eq!(key.len(), 32); // 256 bits / 8
    }
}
