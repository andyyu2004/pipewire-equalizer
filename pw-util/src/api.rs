use std::ffi::CString;

use pipewire::context::ContextRc;
pub use pipewire::module::ModuleInfo;
use pipewire_sys::pw_impl_module;

pub fn load_module(context: &ContextRc, name: &str, args: &str) -> anyhow::Result<ImplModule> {
    let name_c = CString::new(name).unwrap();
    let args = CString::new(args).unwrap();
    let module = unsafe {
        pipewire_sys::pw_context_load_module(
            context.as_raw_ptr(),
            name_c.as_ptr(),
            args.as_ptr(),
            std::ptr::null_mut(),
        )
    };

    if module.is_null() {
        anyhow::bail!("Failed to load module: {name}");
    }

    Ok(ImplModule(module))
}

pub struct ImplModule(*mut pw_impl_module);

impl ImplModule {
    pub fn info(&self) -> ModuleInfo {
        let ptr = unsafe { pipewire_sys::pw_impl_module_get_info(self.0) };
        ModuleInfo::new(std::ptr::NonNull::new(ptr.cast_mut()).expect("module info is NULL"))
    }
}

impl Drop for ImplModule {
    fn drop(&mut self) {
        unsafe {
            pipewire_sys::pw_impl_module_destroy(self.0);
        }
    }
}
