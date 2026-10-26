# Encryption Support

## Overview

Microsoft Access databases use different encryption schemes depending on the file format:

- **.mdb files** (Access 97-2003): Use Jet RC4 encryption. The encryption key is derived from the database header and does not require a password to read. jetdb handles this transparently.
- **.accdb files** (Access 2007+): May use one of several Office encryption schemes, all of which require a password.

## EncryptionInfo Version Detection

For .accdb files, page 0 stores the length of the EncryptionInfo (u16 LE) at offset 0x299, and the EncryptionInfo follows it at offset 0x29B. The first 4 bytes of the EncryptionInfo contain `vMajor` (u16 LE) and `vMinor` (u16 LE), which determine the encryption scheme:

| vMajor | vMinor | Scheme | Status |
|--------|--------|--------|--------|
| 4 | 4 | Agile Encryption | Supported |
| 2/3/4 | 2 | RC4 CryptoAPI | Supported |
| 2/3/4 | 2 (AES flag) | Standard AES | Supported |
| 2/3/4 | 2 (AES algId) | NonStandard AES | Supported |
| 3/4 | 3 | Extensible Encryption | Not supported |
| 1 | 1 | Office Binary Doc RC4 | Not supported |
| - | - | Jet RC4 (.mdb) | No password needed |

For vMajor=2/3/4, vMinor=2: The flags field (offset 4 in EncryptionInfo) determines the sub-type:
- `FCRYPTO_API_FLAG (0x04)` set + `FAES_FLAG (0x20)` set: Standard AES
- `FCRYPTO_API_FLAG (0x04)` set + `FAES_FLAG (0x20)` not set: RC4 CryptoAPI or NonStandard AES (determined by algId in the EncryptionHeader)

## Algorithm Details

### Agile Encryption (vMajor=4, vMinor=4)

Used by Access 2010+ (default). Parameters are stored as XML in the EncryptionInfo.

- **Key derivation**: PBKDF2-like iterated hash (configurable algorithm: SHA-1/256/384/512)
- **Page encryption**: AES-CBC with per-page IV derived from `hash(salt + page_key)`
- **Spec reference**: MS-OFFCRYPTO Section 2.3.4.10

### RC4 CryptoAPI (vMajor=2/3/4, vMinor=2)

Used by some Access 2007 files.

- **Key derivation**: `base_hash = SHA1(salt + password_UTF16LE)`, then `enc_key = SHA1(base_hash + block_bytes)` truncated to key_size/8 bytes
- **Page encryption**: RC4 stream cipher with per-page key
- **Key sizes**: 40-bit (zero-padded to 128-bit) or 128-bit
- **Spec reference**: MS-OFFCRYPTO Sections 2.3.5.2, 2.3.5.4

### NonStandard AES (vMajor=2/3/4, vMinor=2, AES algId)

A variant that uses AES encryption but with non-standard parameters (0 hash iterations).

- **Key derivation**: Same as Standard AES but with `hash_iterations = 0` (no iterative hashing)
- **Page encryption**: AES-ECB
- **Key derivation function**: `cryptDeriveKey` using HMAC-like XOR construction (`genXBytes`)
- **Key sizes**: 128, 192, or 256 bits

### Standard AES (vMajor=2/3/4, vMinor=2, AES flag)

- **Key derivation**: Same as NonStandard AES but with `hash_iterations = 50000`
- **Page encryption**: AES-ECB
- **Spec reference**: MS-OFFCRYPTO Sections 2.3.4.7, 2.3.4.9

## Usage

```rust
use jetdb::PageReader;

// Password-protected .accdb file
let reader = PageReader::open_with_password("database.accdb", Some("password"))?;

// A file that needs no password: a .mdb file, or an .accdb file without encryption
let reader = PageReader::open("database.mdb")?;
```

The encryption scheme is detected automatically. If a password is required but not provided, `FileError::PasswordRequired` is returned. If the password is incorrect, `FileError::InvalidPassword` is returned.
