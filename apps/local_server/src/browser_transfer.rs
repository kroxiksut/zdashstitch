// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Extension, Json, Router,
    body::{Body, Bytes},
    extract::Path,
    http::{
        HeaderValue, Method, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization as _;
use zdashstitch_application::MessageId;

use crate::{AttachmentResponse, LocalSecurity};

const TRANSFER_TTL: Duration = Duration::from_secs(10 * 60);
pub(crate) const MAX_FILES: usize = 32;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_UPLOAD_SETS: usize = 8;
const MAX_DOWNLOADS: usize = 8;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const REDACTED: &str = "<redacted>";

#[derive(Clone)]
pub struct BrowserTransferService {
    inner: Arc<TransferInner>,
}

struct TransferInner {
    state: Mutex<TransferState>,
    ttl: Duration,
}

#[derive(Default)]
struct TransferState {
    upload_sets: HashMap<String, UploadSet>,
    downloads: HashMap<String, DownloadRecord>,
}

struct UploadSet {
    files: BTreeMap<String, Bytes>,
    phase: UploadPhase,
    /// ASCII-case-folded paths. No Unicode normalization crate is in the
    /// dependency tree, so names that differ only by Unicode form or by
    /// non-ASCII case are still treated as distinct.
    folded_paths: BTreeSet<String>,
    total_bytes: usize,
    expires_at: Instant,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum UploadPhase {
    Open,
    Inspecting,
    Inspected,
}

struct DownloadRecord {
    display_name: String,
    bytes: Vec<u8>,
    expires_at: Instant,
}

#[derive(Clone, Eq, PartialEq)]
pub struct UploadedFile {
    pub relative_path: String,
    pub bytes: Bytes,
}

impl fmt::Debug for UploadedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadedFile")
            .field("relative_path", &self.relative_path)
            .field("size_bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct UploadSetHandle {
    pub id: String,
    pub expires_in_seconds: u64,
}

impl fmt::Debug for UploadSetHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadSetHandle")
            .field("id", &REDACTED)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct UploadReceipt {
    pub upload_set_id: String,
    pub relative_path: String,
    pub size_bytes: usize,
    pub file_count: usize,
    pub total_bytes: usize,
}

impl fmt::Debug for UploadReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadReceipt")
            .field("upload_set_id", &REDACTED)
            .field("relative_path", &self.relative_path)
            .field("size_bytes", &self.size_bytes)
            .field("file_count", &self.file_count)
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct DownloadHandle {
    pub id: String,
    pub display_name: String,
    pub expires_in_seconds: u64,
}

impl fmt::Debug for DownloadHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadHandle")
            .field("id", &REDACTED)
            .field("display_name", &self.display_name)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferError {
    InvalidOrExpired,
    InvalidRelativePath,
    DuplicateFile,
    TooManyFiles,
    TooManyTransfers,
    FileTooLarge,
    TotalTooLarge,
    RandomSource,
}

impl TransferError {
    fn status(self) -> StatusCode {
        match self {
            Self::InvalidOrExpired => StatusCode::NOT_FOUND,
            Self::InvalidRelativePath | Self::DuplicateFile => StatusCode::BAD_REQUEST,
            Self::TooManyFiles | Self::TooManyTransfers => StatusCode::TOO_MANY_REQUESTS,
            Self::FileTooLarge | Self::TotalTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RandomSource => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn message_id(self) -> MessageId {
        match self {
            Self::InvalidOrExpired => MessageId::HTTP_TRANSFER_INVALID,
            Self::InvalidRelativePath => MessageId::HTTP_TRANSFER_INVALID_PATH,
            Self::DuplicateFile => MessageId::HTTP_TRANSFER_DUPLICATE_FILE,
            Self::TooManyFiles => MessageId::HTTP_SECURITY_UPLOAD_COUNT_EXCEEDED,
            Self::TooManyTransfers => MessageId::HTTP_TRANSFER_TOO_MANY,
            Self::FileTooLarge => MessageId::HTTP_SECURITY_BODY_TOO_LARGE,
            Self::TotalTooLarge => MessageId::HTTP_TRANSFER_TOTAL_TOO_LARGE,
            Self::RandomSource => MessageId::HTTP_TRANSFER_UNAVAILABLE,
        }
    }
}

#[derive(Serialize)]
struct TransferErrorBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl BrowserTransferService {
    #[must_use]
    pub fn new() -> Self {
        Self::with_ttl(TRANSFER_TTL)
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(TransferInner {
                state: Mutex::new(TransferState::default()),
                ttl,
            }),
        }
    }

    pub fn create_upload_set(&self) -> Result<UploadSetHandle, TransferError> {
        let mut state = self.lock_and_prune();
        if state.upload_sets.len() >= MAX_UPLOAD_SETS {
            return Err(TransferError::TooManyTransfers);
        }
        let id = unique_id(&state)?;
        state.upload_sets.insert(
            id.clone(),
            UploadSet {
                files: BTreeMap::new(),
                phase: UploadPhase::Open,
                folded_paths: BTreeSet::new(),
                total_bytes: 0,
                expires_at: Instant::now() + self.inner.ttl,
            },
        );
        Ok(UploadSetHandle {
            id,
            expires_in_seconds: self.inner.ttl.as_secs(),
        })
    }

    /// Checks, before the request body is read, that the upload set exists,
    /// still accepts files and has quota for the declared body length.
    /// Returns the maximum number of body bytes worth reading. `add_file`
    /// repeats every check under the lock, so this is only an early reject.
    pub(crate) fn upload_capacity(
        &self,
        upload_set_id: &str,
        declared_length: Option<usize>,
    ) -> Result<usize, TransferError> {
        let state = self.lock_and_prune();
        let retained_bytes = state.retained_bytes();
        let set = state
            .upload_sets
            .get(upload_set_id)
            .filter(|set| set.phase == UploadPhase::Open)
            .ok_or(TransferError::InvalidOrExpired)?;
        if set.files.len() >= MAX_FILES {
            return Err(TransferError::TooManyFiles);
        }
        if declared_length.is_some_and(|length| length > MAX_FILE_BYTES) {
            return Err(TransferError::FileTooLarge);
        }
        let capacity = MAX_FILE_BYTES
            .min(MAX_TOTAL_BYTES.saturating_sub(set.total_bytes))
            .min(MAX_RETAINED_BYTES.saturating_sub(retained_bytes));
        if declared_length.is_some_and(|length| length > capacity) {
            return Err(TransferError::TotalTooLarge);
        }
        Ok(capacity)
    }

    pub fn add_file(
        &self,
        upload_set_id: &str,
        relative_path: &str,
        bytes: Bytes,
    ) -> Result<UploadReceipt, TransferError> {
        validate_relative_path(relative_path)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(TransferError::FileTooLarge);
        }
        let folded_path = fold_for_duplicates(relative_path);
        let mut state = self.lock_and_prune();
        let retained_bytes = state.retained_bytes();
        let set = state
            .upload_sets
            .get_mut(upload_set_id)
            .filter(|set| set.phase == UploadPhase::Open)
            .ok_or(TransferError::InvalidOrExpired)?;
        if set.files.contains_key(relative_path) || set.folded_paths.contains(&folded_path) {
            return Err(TransferError::DuplicateFile);
        }
        if set.files.len() >= MAX_FILES {
            return Err(TransferError::TooManyFiles);
        }
        let total_bytes = set
            .total_bytes
            .checked_add(bytes.len())
            .ok_or(TransferError::TotalTooLarge)?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(TransferError::TotalTooLarge);
        }
        if retained_bytes
            .checked_add(bytes.len())
            .is_none_or(|total| total > MAX_RETAINED_BYTES)
        {
            return Err(TransferError::TotalTooLarge);
        }
        let size_bytes = bytes.len();
        set.files.insert(relative_path.to_owned(), bytes);
        set.folded_paths.insert(folded_path);
        set.total_bytes = total_bytes;
        set.expires_at = Instant::now() + self.inner.ttl;
        Ok(UploadReceipt {
            upload_set_id: upload_set_id.to_owned(),
            relative_path: relative_path.to_owned(),
            size_bytes,
            file_count: set.files.len(),
            total_bytes,
        })
    }

    pub fn take_upload_set(&self, id: &str) -> Result<Vec<UploadedFile>, TransferError> {
        self.take_set(id, UploadPhase::Open)
    }

    /// Freezes the exact file selection presented for preview. Bytes are
    /// shared with the existing quota-bound upload set until confirmation.
    pub(crate) fn seal_upload_set(&self, id: &str) -> Result<Vec<UploadedFile>, TransferError> {
        let mut state = self.lock_and_prune();
        let set = state
            .upload_sets
            .get_mut(id)
            .filter(|set| set.phase == UploadPhase::Open && !set.files.is_empty())
            .ok_or(TransferError::InvalidOrExpired)?;
        set.phase = UploadPhase::Inspecting;
        Ok(set
            .files
            .iter()
            .map(|(relative_path, bytes)| UploadedFile {
                relative_path: relative_path.clone(),
                bytes: bytes.clone(),
            })
            .collect())
    }

    pub(crate) fn complete_inspection(&self, id: &str) -> Result<(), TransferError> {
        let mut state = self.lock_and_prune();
        let set = state
            .upload_sets
            .get_mut(id)
            .filter(|set| set.phase == UploadPhase::Inspecting)
            .ok_or(TransferError::InvalidOrExpired)?;
        set.phase = UploadPhase::Inspected;
        set.expires_at = Instant::now() + self.inner.ttl;
        Ok(())
    }

    pub(crate) fn take_sealed_upload_set(
        &self,
        id: &str,
    ) -> Result<Vec<UploadedFile>, TransferError> {
        self.take_set(id, UploadPhase::Inspected)
    }

    fn take_set(&self, id: &str, phase: UploadPhase) -> Result<Vec<UploadedFile>, TransferError> {
        let mut state = self.lock_and_prune();
        if !state
            .upload_sets
            .get(id)
            .is_some_and(|set| set.phase == phase)
        {
            return Err(TransferError::InvalidOrExpired);
        }
        let set = state
            .upload_sets
            .remove(id)
            .ok_or(TransferError::InvalidOrExpired)?;
        Ok(set
            .files
            .into_iter()
            .map(|(relative_path, bytes)| UploadedFile {
                relative_path,
                bytes,
            })
            .collect())
    }

    pub fn register_download(
        &self,
        display_name: &str,
        bytes: Vec<u8>,
    ) -> Result<DownloadHandle, TransferError> {
        validate_display_name(display_name)?;
        if bytes.len() > MAX_TOTAL_BYTES {
            return Err(TransferError::TotalTooLarge);
        }
        let mut state = self.lock_and_prune();
        if state.downloads.len() >= MAX_DOWNLOADS {
            return Err(TransferError::TooManyTransfers);
        }
        if state
            .retained_bytes()
            .checked_add(bytes.len())
            .is_none_or(|total| total > MAX_RETAINED_BYTES)
        {
            return Err(TransferError::TotalTooLarge);
        }
        let id = unique_id(&state)?;
        state.downloads.insert(
            id.clone(),
            DownloadRecord {
                display_name: display_name.to_owned(),
                bytes,
                expires_at: Instant::now() + self.inner.ttl,
            },
        );
        Ok(DownloadHandle {
            id,
            display_name: display_name.to_owned(),
            expires_in_seconds: self.inner.ttl.as_secs(),
        })
    }

    /// Drops expired upload sets and downloads, so their memory is released
    /// even when nobody uses those IDs again.
    pub(crate) fn prune_expired(&self) {
        drop(self.lock_and_prune());
    }

    fn take_download(&self, id: &str) -> Result<DownloadRecord, TransferError> {
        self.lock_and_prune()
            .downloads
            .remove(id)
            .ok_or(TransferError::InvalidOrExpired)
    }

    /// Reads download metadata for `HEAD` without consuming the one-time record.
    fn peek_download(&self, id: &str) -> Result<(String, usize), TransferError> {
        self.lock_and_prune()
            .downloads
            .get(id)
            .map(|download| (download.display_name.clone(), download.bytes.len()))
            .ok_or(TransferError::InvalidOrExpired)
    }

    fn lock_and_prune(&self) -> std::sync::MutexGuard<'_, TransferState> {
        let now = Instant::now();
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.upload_sets.retain(|_, set| set.expires_at > now);
        state
            .downloads
            .retain(|_, download| download.expires_at > now);
        state
    }
}

impl TransferState {
    fn retained_bytes(&self) -> usize {
        self.upload_sets
            .values()
            .map(|set| set.total_bytes)
            .chain(self.downloads.values().map(|download| download.bytes.len()))
            .sum()
    }
}

impl Default for BrowserTransferService {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/upload-sets", post(create_upload_set))
        .route(
            "/uploads/{set_id}/files/{*relative_path}",
            post(upload_file),
        )
        .route("/downloads/{id}", get(download))
}

/// Returns the upload set ID when `path` (relative to `/api`) addresses the
/// built-in upload route.
pub(crate) fn upload_route_set_id(path: &str) -> Option<&str> {
    let (set_id, tail) = path.strip_prefix("/uploads/")?.split_once('/')?;
    (!set_id.is_empty() && tail.starts_with("files/")).then_some(set_id)
}

async fn create_upload_set(Extension(service): Extension<BrowserTransferService>) -> Response {
    match service.create_upload_set() {
        Ok(handle) => (StatusCode::CREATED, Json(handle)).into_response(),
        Err(error) => transfer_error(error),
    }
}

async fn upload_file(
    Extension(service): Extension<BrowserTransferService>,
    Path((set_id, relative_path)): Path<(String, String)>,
    bytes: Bytes,
) -> Response {
    match service.add_file(&set_id, &relative_path, bytes) {
        Ok(receipt) => (StatusCode::CREATED, Json(receipt)).into_response(),
        Err(error) => transfer_error(error),
    }
}

async fn download(
    Extension(service): Extension<BrowserTransferService>,
    method: Method,
    Path(id): Path<String>,
) -> Response {
    let result = if method == Method::HEAD {
        service
            .peek_download(&id)
            .map(|(display_name, size)| (display_name, size, Body::empty()))
    } else {
        service.take_download(&id).map(|download| {
            let size = download.bytes.len();
            (download.display_name, size, Body::from(download.bytes))
        })
    };
    match result {
        Ok((display_name, size, body)) => {
            let mut response = Response::new(body);
            let headers = response.headers_mut();
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            headers.insert(CONTENT_LENGTH, HeaderValue::from(size));
            response
                .extensions_mut()
                .insert(AttachmentResponse::named(display_name));
            response
        }
        Err(error) => transfer_error(error),
    }
}

pub(crate) fn transfer_error(error: TransferError) -> Response {
    (
        error.status(),
        Json(TransferErrorBody {
            code: error.message_id().as_str(),
            params: BTreeMap::new(),
        }),
    )
        .into_response()
}

/// Comparison key for duplicate detection (revised S-1, 2026-10-25): NFC,
/// full Unicode lowercase (Cyrillic included) and final sigma folded to sigma,
/// then NFC again, so visually identical names, including NFC and NFD forms of
/// one name, count as one file. The stored path stays unchanged.
fn fold_for_duplicates(path: &str) -> String {
    path.nfc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character == '\u{3c2}' {
                '\u{3c3}'
            } else {
                character
            }
        })
        .nfc()
        .collect()
}

fn validate_relative_path(value: &str) -> Result<(), TransferError> {
    if value.is_empty() || value.len() > 1024 || value.starts_with('/') || value.contains('\\') {
        return Err(TransferError::InvalidRelativePath);
    }
    let segments: Vec<_> = value.split('/').collect();
    if segments.len() > 16 || !segments.iter().all(|segment| portable_segment(segment)) {
        return Err(TransferError::InvalidRelativePath);
    }
    Ok(())
}

/// A path segment that can be materialized on Windows, macOS and Linux
/// without being reinterpreted: no control, format (including bidi
/// overrides) or Windows-reserved characters, no trailing dot or space, and
/// no reserved DOS device name.
fn portable_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.ends_with(['.', ' '])
        && !segment.chars().any(|character| {
            is_control_or_format(character)
                || matches!(character, ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
        && !is_windows_reserved_name(segment)
}

fn is_windows_reserved_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|rest| {
            let mut characters = rest.chars();
            matches!(
                (characters.next(), characters.next()),
                (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        })
    })
}

/// Unicode general categories Cc and Cf. `std` has no Cf helper, so the Cf
/// code points (Unicode 16) are listed explicitly.
fn is_control_or_format(character: char) -> bool {
    const FORMAT_RANGES: &[(u32, u32)] = &[
        (0x00AD, 0x00AD),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0001, 0xE0001),
        (0xE0020, 0xE007F),
    ];
    let code = u32::from(character);
    character.is_control()
        || FORMAT_RANGES
            .iter()
            .any(|(first, last)| (*first..=*last).contains(&code))
}

/// Download names end up in `Content-Disposition`, so characters that could
/// break or spoof the header value are rejected up front.
pub(crate) fn validate_display_name(value: &str) -> Result<(), TransferError> {
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.chars().any(|character| {
            is_control_or_format(character) || matches!(character, '/' | '\\' | '"' | ';')
        })
    {
        Err(TransferError::InvalidRelativePath)
    } else {
        Ok(())
    }
}

fn random_id() -> Result<String, TransferError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| TransferError::RandomSource)?;
    let mut id = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}

fn unique_id(state: &TransferState) -> Result<String, TransferError> {
    loop {
        let id = random_id()?;
        if !state.upload_sets.contains_key(&id) && !state.downloads.contains_key(&id) {
            return Ok(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn upload_sets_preserve_safe_relative_paths_and_are_consumed_once() {
        let service = BrowserTransferService::new();
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        let receipt = service
            .add_file(&set.id, "templates/db.yaml", Bytes::from_static(b"safe"))
            .unwrap_or_else(|error| panic!("add file: {error:?}"));
        assert_eq!(receipt.file_count, 1);
        assert_eq!(
            service.add_file(
                &set.id,
                "templates/db.yaml",
                Bytes::from_static(b"duplicate")
            ),
            Err(TransferError::DuplicateFile)
        );
        assert_eq!(
            service.take_upload_set(&set.id),
            Ok(vec![UploadedFile {
                relative_path: "templates/db.yaml".to_owned(),
                bytes: Bytes::from_static(b"safe"),
            }])
        );
        assert_eq!(
            service.take_upload_set(&set.id),
            Err(TransferError::InvalidOrExpired)
        );
    }

    #[test]
    fn traversal_absolute_and_cross_platform_separator_paths_are_rejected() {
        let service = BrowserTransferService::new();
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        for path in ["../secret", "/absolute", "folder\\secret", "a//b", "./file"] {
            assert_eq!(
                service.add_file(&set.id, path, Bytes::from_static(b"x")),
                Err(TransferError::InvalidRelativePath)
            );
        }
    }

    #[test]
    fn non_portable_path_segments_are_rejected() {
        let service = BrowserTransferService::new();
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        for path in [
            "C:/windows.yaml",
            "folder/stream.yaml:hidden",
            "trailing-dot./db.yaml",
            "db.yaml ",
            "templates/db.yaml.",
            "CON",
            "templates/nul.yaml",
            "Com1.txt",
            "lpt9",
            "LPT\u{b9}.yaml",
            "conin$.yaml",
            "line\nbreak.yaml",
            "tab\there.yaml",
            "bidi\u{202e}lmay.exe",
            "isolate\u{2066}.yaml",
            "zero\u{200b}width.yaml",
            "bom\u{feff}.yaml",
            "question?.yaml",
            "star*.yaml",
            "pipe|.yaml",
            "angle<.yaml",
            "quote\".yaml",
            "nul\0byte.yaml",
        ] {
            assert_eq!(
                service.add_file(&set.id, path, Bytes::from_static(b"x")),
                Err(TransferError::InvalidRelativePath),
                "{path:?} must be rejected"
            );
        }
        for path in [
            "console.yaml",
            "COM10.yaml",
            "connections/db.yaml",
            "шаблоны/postgres.yaml",
            "v1.2/db.yaml",
        ] {
            assert!(
                service
                    .add_file(&set.id, path, Bytes::from_static(b"x"))
                    .is_ok(),
                "{path:?} must be accepted"
            );
        }
    }

    #[test]
    fn duplicates_are_detected_after_nfc_and_unicode_case_folding() {
        let service = BrowserTransferService::new();
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        for (first, duplicate) in [
            ("Templates/DB.yaml", "templates/db.YAML"),
            ("Панель.yaml", "панель.yaml"),
            ("ШАБЛОНЫ/Ёлка.yaml", "шаблоны/ёлка.yaml"),
            ("\u{439}од.yaml", "\u{438}\u{306}од.yaml"),
            ("ΟΔΟΣ.yaml", "οδος.yaml"),
        ] {
            service
                .add_file(&set.id, first, Bytes::from_static(b"x"))
                .unwrap_or_else(|error| panic!("add {first:?}: {error:?}"));
            assert_eq!(
                service.add_file(&set.id, duplicate, Bytes::from_static(b"x")),
                Err(TransferError::DuplicateFile),
                "{duplicate:?} duplicates {first:?}"
            );
        }
        for distinct in ["панели.yaml", "и\u{306}од.yml", "Панель-2.yaml"] {
            assert!(
                service
                    .add_file(&set.id, distinct, Bytes::from_static(b"x"))
                    .is_ok(),
                "{distinct:?} is not a duplicate"
            );
        }
    }

    #[test]
    fn upload_capacity_is_checked_before_the_body_is_read() {
        let service = BrowserTransferService::new();
        assert_eq!(
            service.upload_capacity("missing", Some(1)),
            Err(TransferError::InvalidOrExpired)
        );
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        assert_eq!(service.upload_capacity(&set.id, None), Ok(MAX_FILE_BYTES));
        assert_eq!(
            service.upload_capacity(&set.id, Some(MAX_FILE_BYTES + 1)),
            Err(TransferError::FileTooLarge)
        );
        for index in 0..2 {
            service
                .add_file(
                    &set.id,
                    &format!("large-{index}.bin"),
                    Bytes::from(vec![0_u8; MAX_FILE_BYTES - 1]),
                )
                .unwrap_or_else(|error| panic!("add large file: {error:?}"));
        }
        assert_eq!(service.upload_capacity(&set.id, None), Ok(2));
        assert_eq!(
            service.upload_capacity(&set.id, Some(3)),
            Err(TransferError::TotalTooLarge)
        );
    }

    #[test]
    fn expired_uploads_and_downloads_are_not_reused() {
        let service = BrowserTransferService::with_ttl(Duration::from_millis(1));
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        let download = service
            .register_download("draft.zip", b"safe".to_vec())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));
        thread::sleep(Duration::from_millis(5));
        assert_eq!(
            service.take_upload_set(&set.id),
            Err(TransferError::InvalidOrExpired)
        );
        assert!(matches!(
            service.take_download(&download.id),
            Err(TransferError::InvalidOrExpired)
        ));
    }

    #[test]
    fn pruning_releases_expired_records_without_touching_their_ids() {
        let service = BrowserTransferService::with_ttl(Duration::from_millis(1));
        service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        service
            .register_download("draft.zip", vec![0_u8; 1024])
            .unwrap_or_else(|error| panic!("register download: {error:?}"));
        thread::sleep(Duration::from_millis(5));
        service.prune_expired();
        let state = service
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(state.upload_sets.is_empty());
        assert!(state.downloads.is_empty());
        assert_eq!(state.retained_bytes(), 0);
    }

    #[test]
    fn head_style_peek_does_not_consume_a_download() {
        let service = BrowserTransferService::new();
        let download = service
            .register_download("draft.zip", b"safe".to_vec())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));
        assert_eq!(
            service.peek_download(&download.id),
            Ok(("draft.zip".to_owned(), 4))
        );
        assert!(service.take_download(&download.id).is_ok());
        assert!(matches!(
            service.take_download(&download.id),
            Err(TransferError::InvalidOrExpired)
        ));
    }

    #[test]
    fn display_names_that_could_break_headers_are_rejected() {
        let service = BrowserTransferService::new();
        for name in [
            "",
            ".",
            "..",
            "a/b.zip",
            "a\\b.zip",
            "quote\".zip",
            "semi;colon.zip",
            "line\r\nbreak.zip",
            "tab\t.zip",
            "bidi\u{202e}piz.exe",
        ] {
            assert_eq!(
                service.register_download(name, Vec::new()),
                Err(TransferError::InvalidRelativePath),
                "{name:?} must be rejected"
            );
        }
        assert!(
            service
                .register_download("Панель Prod.zip", Vec::new())
                .is_ok()
        );
    }

    #[test]
    fn debug_output_masks_ids_and_file_contents() {
        let service = BrowserTransferService::new();
        let set = service
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        let receipt = service
            .add_file(&set.id, "db.yaml", Bytes::from_static(b"secret-content"))
            .unwrap_or_else(|error| panic!("add file: {error:?}"));
        let download = service
            .register_download("draft.zip", Vec::new())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));
        let files = service
            .take_upload_set(&set.id)
            .unwrap_or_else(|error| panic!("take set: {error:?}"));
        let rendered = format!("{set:?} {receipt:?} {download:?} {files:?}");
        assert!(!rendered.contains(&set.id));
        assert!(!rendered.contains(&download.id));
        assert!(!rendered.contains("secret-content"));
        assert!(rendered.contains("db.yaml"));
        assert!(rendered.contains("draft.zip"));
    }

    #[test]
    fn concurrent_transfer_records_are_bounded() {
        let service = BrowserTransferService::new();
        for _ in 0..MAX_UPLOAD_SETS {
            service
                .create_upload_set()
                .unwrap_or_else(|error| panic!("create bounded set: {error:?}"));
        }
        assert_eq!(
            service.create_upload_set(),
            Err(TransferError::TooManyTransfers)
        );

        for index in 0..MAX_DOWNLOADS {
            service
                .register_download(&format!("draft-{index}.zip"), Vec::new())
                .unwrap_or_else(|error| panic!("register bounded download: {error:?}"));
        }
        assert_eq!(
            service.register_download("one-too-many.zip", Vec::new()),
            Err(TransferError::TooManyTransfers)
        );
    }

    #[test]
    fn upload_route_set_id_matches_only_the_built_in_route() {
        assert_eq!(
            upload_route_set_id("/uploads/abc/files/templates/db.yaml"),
            Some("abc")
        );
        assert_eq!(upload_route_set_id("/uploads/file"), None);
        assert_eq!(upload_route_set_id("/uploads//files/db.yaml"), None);
        assert_eq!(upload_route_set_id("/downloads/abc"), None);
    }
}
