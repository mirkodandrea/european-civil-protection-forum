//! PDF documents a player attaches to a response (the plan or law the
//! decision rests on).
//!
//! Bytes live in memory next to the exercise (never in the serialised views,
//! the event log or the history export: those carry only [`FileRef`]). They are
//! not part of a turn snapshot: a rollback marks the proposals as rolled back
//! but keeps the messages and their documents as history, like the messages
//! themselves. Only EXCON and the author may read a document; see
//! [`Exercise::file_for`].
use crate::Exercise;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Largest accepted PDF.
pub const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
/// Documents one role may have stored, and in total.
pub const MAX_FILES_PER_ROLE: usize = 20;
pub const MAX_FILES: usize = 100;
pub const PDF_CONTENT_TYPE: &str = "application/pdf";

/// An uploaded file as received: content type as declared by the client.
#[derive(Debug, Clone)]
pub struct PdfUpload {
    pub name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// What views and logs carry about a stored document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRef {
    pub id: u64,
    pub name: String,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct StoredFile {
    pub meta: FileRef,
    pub owner: String,
    pub content_type: String,
    pub bytes: Arc<Vec<u8>>,
}

/// Documents and the proposal each one supports.
#[derive(Debug, Default)]
pub struct FileStore {
    files: BTreeMap<u64, StoredFile>,
    by_proposal: BTreeMap<u64, u64>,
}

/// File name without any path, control characters or excess length.
fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let name: String = base
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
        .trim()
        .to_string();
    if name.is_empty() {
        "document.pdf".into()
    } else {
        name
    }
}

impl PdfUpload {
    /// Size, declared type and magic number; nothing is parsed further.
    pub fn validate(&self) -> Result<()> {
        if self.bytes.is_empty() {
            bail!("the attached file is empty");
        }
        if self.bytes.len() > MAX_PDF_BYTES {
            bail!("the attached PDF exceeds {} MB", MAX_PDF_BYTES / 1024 / 1024);
        }
        let declared = self.content_type.split(';').next().unwrap_or("").trim();
        if !declared.eq_ignore_ascii_case(PDF_CONTENT_TYPE) {
            bail!("only PDF documents can be attached");
        }
        if !self.bytes[..self.bytes.len().min(1024)]
            .windows(5)
            .any(|w| w == b"%PDF-")
        {
            bail!("the attached file is not a PDF");
        }
        Ok(())
    }
}

impl Exercise {
    /// Check an upload against the limits before anything else is stored.
    pub fn validate_document(&self, role: &str, upload: &PdfUpload) -> Result<()> {
        upload.validate()?;
        let own = self.files.files.values().filter(|f| f.owner == role).count();
        if own >= MAX_FILES_PER_ROLE || self.files.files.len() >= MAX_FILES {
            bail!("too many documents attached to this exercise");
        }
        Ok(())
    }

    /// Keep `upload` as the document supporting proposal `pid`.
    pub(crate) fn store_document(
        &mut self,
        role: &str,
        pid: u64,
        upload: PdfUpload,
    ) -> Result<FileRef> {
        self.validate_document(role, &upload)?;
        let id = self.id();
        let meta = FileRef {
            id,
            name: clean_name(&upload.name),
            size: upload.bytes.len(),
        };
        self.files.files.insert(
            id,
            StoredFile {
                meta: meta.clone(),
                owner: role.into(),
                content_type: PDF_CONTENT_TYPE.into(),
                bytes: Arc::new(upload.bytes),
            },
        );
        self.files.by_proposal.insert(pid, id);
        Ok(meta)
    }

    /// The document that supports proposal `pid`, if any.
    pub fn document_of(&self, pid: u64) -> Option<&FileRef> {
        let id = self.files.by_proposal.get(&pid)?;
        self.files.files.get(id).map(|f| &f.meta)
    }

    /// A stored document for `reader`: `None` is EXCON (reads everything), a
    /// role reads only what it attached itself.
    pub fn file_for(&self, reader: Option<&str>, id: u64) -> Result<&StoredFile> {
        match self.files.files.get(&id) {
            Some(f) if reader.is_none_or(|r| r == f.owner) => Ok(f),
            // The same answer for "missing" and "not yours": no probing.
            _ => bail!("document not found"),
        }
    }
}
