const ES_CONTINUOUS: u32 = 0x8000_0000;
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

unsafe extern "system" {
    pub fn SetThreadExecutionState(esFlags: u32) -> u32;
}

pub fn inhibit() -> u32 {
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) }
}

pub fn release() {
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS);
    }
}
