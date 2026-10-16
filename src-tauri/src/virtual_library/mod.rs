// Ticket #42 consumes this boundary when it projects placeholders.
pub(crate) mod albums;
#[allow(dead_code)]
pub(crate) mod filenames;
pub(crate) mod hydration;
pub(crate) mod sync_root;
#[allow(dead_code)] // Wired to reconciliation in a follow-up ticket.
pub(crate) mod timeline;
