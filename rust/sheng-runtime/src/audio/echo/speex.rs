//! Safe wrapper over SpeexDSP's linear echo canceller. The non-linear preprocessor is
//! deliberately not used, so near-end speech is never suppressed.

use std::os::raw::{c_int, c_void};
use std::ptr::NonNull;

use aec_rs_sys::{
    speex_echo_cancellation, speex_echo_ctl, speex_echo_state_destroy, speex_echo_state_init,
    SpeexEchoState, SPEEX_ECHO_SET_SAMPLING_RATE,
};

/// Owns one canceller state; frame and filter sizes are fixed at creation.
pub struct SpeexEcho {
    state: NonNull<SpeexEchoState>,
    frame: usize,
}

// SAFETY: the state is only reached through `&mut self`, so it is never used from two
// threads at once; SpeexDSP keeps no thread-local data.
unsafe impl Send for SpeexEcho {}

impl SpeexEcho {
    /// Returns `None` if SpeexDSP cannot allocate the state.
    pub fn new(sample_rate: u32, frame: usize, filter: usize) -> Option<Self> {
        let frame_c = c_int::try_from(frame).ok()?;
        let filter_c = c_int::try_from(filter).ok()?;
        let mut rate = c_int::try_from(sample_rate).ok()?;
        // SAFETY: plain constructor call with positive sizes; the result is null-checked.
        let state = NonNull::new(unsafe { speex_echo_state_init(frame_c, filter_c) })?;
        let request = c_int::try_from(SPEEX_ECHO_SET_SAMPLING_RATE).ok()?;
        // SAFETY: `state` is valid and `rate` lives for the duration of the call.
        let status = unsafe {
            speex_echo_ctl(
                state.as_ptr(),
                request,
                (&mut rate as *mut c_int).cast::<c_void>(),
            )
        };
        let echo = Self { state, frame };
        (status == 0).then_some(echo)
    }

    /// Removes the echo of `reference` from `mic`. All slices hold exactly one frame.
    pub fn cancel(&mut self, mic: &[i16], reference: &[i16], out: &mut [i16]) {
        assert!(
            mic.len() == self.frame && reference.len() == self.frame && out.len() == self.frame
        );
        // SAFETY: the state is valid and each buffer holds `frame` samples, as asserted.
        unsafe {
            speex_echo_cancellation(
                self.state.as_ptr(),
                mic.as_ptr(),
                reference.as_ptr(),
                out.as_mut_ptr(),
            );
        }
    }
}

impl Drop for SpeexEcho {
    fn drop(&mut self) {
        // SAFETY: the state was created by `speex_echo_state_init` and is destroyed once.
        unsafe { speex_echo_state_destroy(self.state.as_ptr()) }
    }
}
