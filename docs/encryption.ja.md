# 暗号化サポート

## 概要

Microsoft Access データベースはファイル形式に応じて異なる暗号化方式を使用します：

- **.mdb ファイル** (Access 97-2003): Jet RC4 暗号化を使用。暗号化キーはデータベースヘッダーから導出され、読み取りにパスワードは不要です。jetdb は透過的に処理します。
- **.accdb ファイル** (Access 2007+): いくつかの Office 暗号化方式のいずれかを使用する場合があり、すべてパスワードが必要です。

## EncryptionInfo バージョン判定

.accdb ファイルでは、EncryptionInfo はページ 0 のオフセット 0x299 に格納されます。先頭 4 バイトに `vMajor` (u16 LE) と `vMinor` (u16 LE) が含まれ、暗号化方式を決定します：

| vMajor | vMinor | 方式 | 状態 |
|--------|--------|------|------|
| 4 | 4 | Agile Encryption | 対応済 |
| 2/3/4 | 2 | RC4 CryptoAPI | 対応済 |
| 2/3/4 | 2+AES | NonStandard AES | 対応済 |
| 3/4 | 3 | Extensible Encryption | 未対応 |
| 1 | 1 | Office Binary Doc RC4 | 未対応 |
| - | - | Jet RC4 (.mdb) | パスワード不要 |

vMajor=2/3/4, vMinor=2 の場合、flags フィールド（EncryptionInfo のオフセット 4）でサブタイプが決まります：
- `FCRYPTO_API_FLAG (0x04)` + `FAES_FLAG (0x20)` の両方がセット: Standard AES
- `FCRYPTO_API_FLAG (0x04)` がセットで `FAES_FLAG (0x20)` がセットでない: RC4 CryptoAPI または NonStandard AES（EncryptionHeader の algId で判定）

## アルゴリズム詳細

### Agile Encryption (vMajor=4, vMinor=4)

Access 2010+ のデフォルト。パラメータは EncryptionInfo 内の XML として格納されます。

- **鍵導出**: PBKDF2 風の反復ハッシュ（SHA-256/384/512 から選択可能）
- **ページ暗号化**: AES-CBC、ページごとに `hash(salt + page_key)` から IV を導出
- **仕様参照**: MS-OFFCRYPTO Section 2.3.4.10

### RC4 CryptoAPI (vMajor=2/3/4, vMinor=2)

一部の Access 2007 ファイルで使用されます。

- **鍵導出**: `base_hash = SHA1(salt + password_UTF16LE)`、次に `enc_key = SHA1(base_hash + block_bytes)` を key_size/8 バイトに切り詰め
- **ページ暗号化**: RC4 ストリーム暗号（ページごとに鍵を導出）
- **鍵サイズ**: 40 ビット（128 ビットにゼロ拡張）または 128 ビット
- **仕様参照**: MS-OFFCRYPTO Sections 2.3.5.2, 2.3.5.4

### NonStandard AES (vMajor=2/3/4, vMinor=2, AES algId)

AES 暗号化を使用しつつ、非標準パラメータ（ハッシュ反復回数 0）のバリアントです。

- **鍵導出**: Standard AES と同一だが `hash_iterations = 0`（反復ハッシュなし）
- **ページ暗号化**: AES-ECB
- **鍵導出関数**: HMAC 風 XOR 構成（`genXBytes`）を使用する `cryptDeriveKey`
- **鍵サイズ**: 128、192、または 256 ビット

### Standard AES (vMajor=3/4, vMinor=2, AES flag)

- **鍵導出**: NonStandard AES と同一だが `hash_iterations = 50000`
- **ページ暗号化**: AES-ECB
- **仕様参照**: MS-OFFCRYPTO Sections 2.3.4.7, 2.3.4.9

## 使い方

```rust
use jetdb::PageReader;

// パスワード保護された .accdb ファイル
let reader = PageReader::open_with_password("database.accdb", Some("password"))?;

// 暗号化されていないファイル（パスワードは無視される）
let reader = PageReader::open("database.mdb")?;
```

暗号化方式は自動的に検出されます。パスワードが必要なのに提供されない場合は `FileError::PasswordRequired`、パスワードが間違っている場合は `FileError::InvalidPassword` が返されます。
