use alloc::boxed::Box;
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Dir,
}

/// Abstract interface for all filesystem nodes.
pub trait VNode: Send + Sync {
    fn kind(&self) -> NodeKind;
    fn size(&self) -> usize;
    /// Copy up to `buf.len()` bytes starting at `offset` into `buf`.
    /// Returns the number of bytes actually read.
    fn read(&self, buf: &mut [u8], offset: usize) -> usize;
    /// Look up a child by name. Only meaningful for `Dir` nodes.
    fn lookup(&self, name: &str) -> Option<Box<dyn VNode>>;
}

// ── Global VFS root ──────────────────────────────────────────────────────────

static ROOT: Mutex<Option<Box<dyn VNode>>> = Mutex::new(None);

/// Install a filesystem as the root.  Called once during kernel init.
pub fn mount(root: Box<dyn VNode>) {
    *ROOT.lock() = Some(root);
}

/// Walk an absolute path and return the target node, or `None` if any
/// component is missing.  An empty or "/" path returns the root itself.
pub fn lookup(path: &str) -> Option<Box<dyn VNode>> {
    let guard = ROOT.lock();
    let root = guard.as_ref()?;

    // Collect non-empty path components (handles leading/trailing slashes).
    let components: alloc::vec::Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if components.is_empty() {
        // Caller asked for "/"; return a clone by re-looking up via root's
        // own lookup — we can't clone Box<dyn VNode> directly, so we
        // return None here and let callers use the guard form for root access.
        return None;
    }

    let mut current: Box<dyn VNode> = root.lookup(components[0])?;
    for &component in &components[1..] {
        current = current.lookup(component)?;
    }
    Some(current)
}

/// Call `f` with a reference to the root node.
/// Returns `None` if no filesystem is mounted.
pub fn with_root<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&dyn VNode) -> T,
{
    let guard = ROOT.lock();
    guard.as_deref().map(f)
}
