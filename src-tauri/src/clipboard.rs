//! Lossless clipboard snapshot/restore (issue #32).
//!
//! `tauri-plugin-clipboard-manager`'s `read_text()` returns `Err` for any
//! non-text clipboard (image, file reference, RTF, …). The injector treated
//! that as "nothing to restore", so dictating while an image or file was on the
//! clipboard silently destroyed it. This module snapshots **every** pasteboard
//! flavor as owned Rust data before injection and writes them all back after,
//! so the user's clipboard survives a paste regardless of its content type.
//!
//! Design notes:
//! * The snapshot is owned `String`/`Vec<u8>` — we deliberately do NOT hold
//!   `Retained<NSData>`/`NSString` across the injector's `.await` points
//!   (activate, paste, sleep). Cocoa objects aren't `Send`, and holding them
//!   would make the inject future non-`Send`; converting to plain bytes also
//!   means a stale autorelease pool can't free data out from under us.
//! * All Objective-C work happens inside `autoreleasepool` blocks.
//! * Promised/lazy flavors (a provider that vends data on demand and hasn't
//!   materialised it) return no bytes; we can't reproduce the promise, so those
//!   flavors are skipped (fail-open: the materialised flavors still restore).
//!   The count is logged so this is observable, never silent.

/// One pasteboard item: its flavors as (UTI type string, raw bytes) pairs.
#[derive(Default)]
pub struct ClipboardSnapshot {
    items: Vec<Vec<(String, Vec<u8>)>>,
    /// Flavors that advertised a type but vended no bytes (promised/lazy).
    pub skipped_promised: usize,
}

impl ClipboardSnapshot {
    /// True when the clipboard held nothing restorable (empty, or only
    /// promised flavors). The injector leaves its pasted text in place in this
    /// case rather than clearing the clipboard.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn flavor_count(&self) -> usize {
        self.items.iter().map(|i| i.len()).sum()
    }
}

#[cfg(target_os = "macos")]
pub fn snapshot() -> ClipboardSnapshot {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    // NSPasteboard is usable from any thread, and objc2 exposes these methods
    // as safe (no MainThreadMarker, no raw pointers).
    autoreleasepool(|_| snapshot_pasteboard(&NSPasteboard::generalPasteboard()))
}

#[cfg(target_os = "macos")]
pub fn restore(snapshot: &ClipboardSnapshot) {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    if snapshot.is_empty() {
        return;
    }
    autoreleasepool(|_| restore_pasteboard(&NSPasteboard::generalPasteboard(), snapshot));
}

/// Core snapshot logic, parameterised by pasteboard so it can be unit-tested
/// against a private `pasteboardWithUniqueName` without touching the user's
/// system clipboard.
#[cfg(target_os = "macos")]
fn snapshot_pasteboard(pb: &objc2_app_kit::NSPasteboard) -> ClipboardSnapshot {
    let mut snap = ClipboardSnapshot::default();
    let Some(items) = pb.pasteboardItems() else {
        return snap;
    };
    for item in items.iter() {
        let mut flavors: Vec<(String, Vec<u8>)> = Vec::new();
        for ty in item.types().iter() {
            match item.dataForType(&ty) {
                Some(data) => flavors.push((ty.to_string(), data.to_vec())),
                None => snap.skipped_promised += 1,
            }
        }
        if !flavors.is_empty() {
            snap.items.push(flavors);
        }
    }
    snap
}

#[cfg(target_os = "macos")]
fn restore_pasteboard(pb: &objc2_app_kit::NSPasteboard, snapshot: &ClipboardSnapshot) {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};

    pb.clearContents();
    let mut objs: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        Vec::with_capacity(snapshot.items.len());
    for flavors in &snapshot.items {
        let item = NSPasteboardItem::new();
        for (uti, bytes) in flavors {
            let data = NSData::with_bytes(bytes);
            let ty = NSString::from_str(uti);
            item.setData_forType(&data, &ty);
        }
        objs.push(ProtocolObject::from_retained(item));
    }
    let array = NSArray::from_retained_slice(&objs);
    pb.writeObjects(&array);
}

#[cfg(not(target_os = "macos"))]
pub fn snapshot() -> ClipboardSnapshot {
    ClipboardSnapshot::default()
}

#[cfg(not(target_os = "macos"))]
pub fn restore(_snapshot: &ClipboardSnapshot) {}

/// The general pasteboard's `changeCount`: a monotonically increasing counter
/// bumped by every pasteboard WRITE (clearContents, writeObjects, another app
/// copying). Reads do not move it. The injector takes a baseline after writing
/// its text and polls this after Cmd+V so the clipboard restore can happen as
/// soon as the pasteboard has moved on, instead of after a fixed worst-case
/// sleep. Off-macOS there is no pasteboard to watch: returns a constant so the
/// poll runs to its cap (the old fixed-sleep behaviour).
#[cfg(target_os = "macos")]
pub fn change_count() -> i64 {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount() as i64)
}

#[cfg(not(target_os = "macos"))]
pub fn change_count() -> i64 {
    0
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;

    fn item(flavors: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        flavors
            .iter()
            .map(|(t, b)| (t.to_string(), b.to_vec()))
            .collect()
    }

    /// Round-trips a multi-flavor clipboard through restore → snapshot against a
    /// PRIVATE pasteboard (`pasteboardWithUniqueName`), so the test never reads
    /// or mutates the user's real system clipboard. Proves the NSPasteboard
    /// FFI preserves arbitrary bytes (incl. non-text / 0xFF) across all flavors.
    #[test]
    fn roundtrip_preserves_all_flavors_on_private_pasteboard() {
        autoreleasepool(|_| {
            let pb = NSPasteboard::pasteboardWithUniqueName();

            let original = ClipboardSnapshot {
                items: vec![
                    item(&[
                        ("public.utf8-plain-text", b"hello wisspa"),
                        // simulate a non-text flavor with raw bytes incl. 0x00/0xFF
                        ("public.png", &[0u8, 1, 2, 250, 255, 0]),
                    ]),
                    item(&[("public.file-url", b"file:///tmp/example.txt")]),
                ],
                skipped_promised: 0,
            };

            restore_pasteboard(&pb, &original);
            let read_back = snapshot_pasteboard(&pb);

            assert_eq!(read_back.item_count(), 2, "both items survive");
            assert_eq!(read_back.flavor_count(), 3, "all flavors survive");

            // Text flavor bytes are byte-identical.
            let text = read_back
                .items
                .iter()
                .flatten()
                .find(|(t, _)| t == "public.utf8-plain-text")
                .map(|(_, b)| b.clone());
            assert_eq!(text.as_deref(), Some(&b"hello wisspa"[..]));

            // Binary flavor bytes (with 0x00 and 0xFF) are byte-identical.
            let png = read_back
                .items
                .iter()
                .flatten()
                .find(|(t, _)| t == "public.png")
                .map(|(_, b)| b.clone());
            assert_eq!(png.as_deref(), Some(&[0u8, 1, 2, 250, 255, 0][..]));
            // The private pasteboard is reclaimed at process exit; no explicit
            // release needed for a test.
        });
    }

    #[test]
    fn empty_snapshot_reports_empty() {
        let snap = ClipboardSnapshot::default();
        assert!(snap.is_empty());
        assert_eq!(snap.flavor_count(), 0);
    }

    /// The injector's early-restore signal rests on writes bumping
    /// changeCount. Proven here against a PRIVATE pasteboard so the test never
    /// touches the user's system clipboard.
    #[test]
    fn writes_bump_change_count_on_private_pasteboard() {
        autoreleasepool(|_| {
            let pb = NSPasteboard::pasteboardWithUniqueName();
            let before = pb.changeCount();
            let snap = ClipboardSnapshot {
                items: vec![item(&[("public.utf8-plain-text", b"wisspa")])],
                skipped_promised: 0,
            };
            restore_pasteboard(&pb, &snap);
            assert!(
                pb.changeCount() > before,
                "clearContents+writeObjects must bump changeCount ({before} -> {})",
                pb.changeCount()
            );
        });
    }
}
