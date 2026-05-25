/// Reads the current value of the system-wide focused UI element via the
/// macOS Accessibility API. Used by the auto-learn edit-capture path.

pub struct FocusedField {
    pub value: String,
    pub role: String,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> *const std::ffi::c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const std::ffi::c_void,
        attribute: *const std::ffi::c_void,
        value: *mut *const std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "macos")]
pub fn focused_field_value() -> Option<FocusedField> {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    if !crate::injector::accessibility_trusted() {
        return None;
    }

    unsafe {
        let sys_raw = AXUIElementCreateSystemWide();
        if sys_raw.is_null() {
            return None;
        }
        let sys = CFType::wrap_under_create_rule(sys_raw as CFTypeRef);

        let attr_focused = CFString::new("AXFocusedUIElement");
        let mut focused_raw: *const c_void = ptr::null();
        if AXUIElementCopyAttributeValue(
            sys.as_CFTypeRef() as *const c_void,
            attr_focused.as_concrete_TypeRef() as *const c_void,
            &mut focused_raw,
        ) != 0
            || focused_raw.is_null()
        {
            return None;
        }
        let focused = CFType::wrap_under_create_rule(focused_raw as CFTypeRef);

        let attr_role = CFString::new("AXRole");
        let mut role_raw: *const c_void = ptr::null();
        if AXUIElementCopyAttributeValue(
            focused.as_CFTypeRef() as *const c_void,
            attr_role.as_concrete_TypeRef() as *const c_void,
            &mut role_raw,
        ) != 0
            || role_raw.is_null()
        {
            return None;
        }
        let role_cf = CFType::wrap_under_create_rule(role_raw as CFTypeRef);
        let role_str = match role_cf.downcast::<CFString>() {
            Some(s) => s.to_string(),
            None => {
                log::debug!("ax_snapshot: AXRole is not a CFString");
                return None;
            }
        };

        if role_str == "AXSecureTextField" {
            log::debug!("ax_snapshot: skipping secure text field");
            return None;
        }

        let attr_value = CFString::new("AXValue");
        let mut value_raw: *const c_void = ptr::null();
        if AXUIElementCopyAttributeValue(
            focused.as_CFTypeRef() as *const c_void,
            attr_value.as_concrete_TypeRef() as *const c_void,
            &mut value_raw,
        ) != 0
            || value_raw.is_null()
        {
            log::debug!("ax_snapshot: no AXValue on focused element (role={role_str})");
            return None;
        }
        let value_cf = CFType::wrap_under_create_rule(value_raw as CFTypeRef);
        let value_str = match value_cf.downcast::<CFString>() {
            Some(s) => s.to_string(),
            None => {
                log::debug!("ax_snapshot: AXValue is not a CFString (role={role_str})");
                return None;
            }
        };

        Some(FocusedField {
            value: value_str,
            role: role_str,
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn focused_field_value() -> Option<FocusedField> {
    None
}
