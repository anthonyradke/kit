//! The APO as an aggregated COM object.
//!
//! audiodg never creates an APO on its own: it creates it inside an outer
//! object of its own (COM aggregation, a non-null `pUnkOuter`), and refusing
//! that leaves the endpoint with no working effect chain, so every app fails
//! to open the mic. windows-rs' `#[implement]` can't be aggregated, so this
//! wraps the implemented object: the IUnknown methods of each APO interface
//! go to the outer object, as aggregation requires, and every other method
//! is forwarded to the inner object as is. A separate, non-delegating
//! IUnknown owns the wrapper's lifetime; only the outer object holds it.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{Interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Media::Audio::Apo::*;

#[repr(C)]
pub struct Aggregate {
    // Interface slots: a pointer to one of these is an interface pointer.
    unknown: *const IUnknown_Vtbl,
    apo: *const IAudioProcessingObject_Vtbl,
    rt: *const IAudioProcessingObjectRT_Vtbl,
    config: *const IAudioProcessingObjectConfiguration_Vtbl,
    effects: *const IAudioSystemEffects_Vtbl,
    refs: AtomicU32,
    /// Not reference counted: the outer object owns us, not the other way round.
    outer: *mut c_void,
    inner_apo: IAudioProcessingObject,
    inner_rt: IAudioProcessingObjectRT,
    inner_config: IAudioProcessingObjectConfiguration,
}

const UNKNOWN: usize = 0;
const APO: usize = 1;
const RT: usize = 2;
const CONFIG: usize = 3;
const EFFECTS: usize = 4;

/// Wrap `inner` (which must implement all the APO interfaces) for `outer`
/// and return the non-delegating IUnknown, with one reference.
pub fn create(inner: &IUnknown, outer: *mut c_void) -> windows::core::Result<*mut c_void> {
    let aggregate = Box::new(Aggregate {
        unknown: &UNKNOWN_VTBL,
        apo: &APO_VTBL,
        rt: &RT_VTBL,
        config: &CONFIG_VTBL,
        effects: &EFFECTS_VTBL,
        refs: AtomicU32::new(1),
        outer,
        inner_apo: inner.cast()?,
        inner_rt: inner.cast()?,
        inner_config: inner.cast()?,
    });
    Ok(Box::into_raw(aggregate) as *mut c_void)
}

/// The wrapper an interface pointer in `SLOT` belongs to.
unsafe fn this<'a, const SLOT: usize>(ptr: *mut c_void) -> &'a Aggregate {
    unsafe { &*((ptr as *const *const c_void).sub(SLOT) as *const Aggregate) }
}

fn slot(a: &Aggregate, n: usize) -> *mut c_void {
    unsafe { (a as *const Aggregate as *const *const c_void).add(n) as *mut c_void }
}

unsafe fn outer_vtbl<'a>(a: &Aggregate) -> &'a IUnknown_Vtbl {
    unsafe { &**(a.outer as *const *const IUnknown_Vtbl) }
}

// ---- non-delegating IUnknown: the wrapper's own identity and lifetime ----

unsafe extern "system" fn inner_query(ptr: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() || iid.is_null() {
        return E_POINTER;
    }
    let a = unsafe { this::<UNKNOWN>(ptr) };
    let iid = unsafe { *iid };
    let n = if iid == IUnknown::IID {
        UNKNOWN
    } else if iid == IAudioProcessingObject::IID {
        APO
    } else if iid == IAudioProcessingObjectRT::IID {
        RT
    } else if iid == IAudioProcessingObjectConfiguration::IID {
        CONFIG
    } else if iid == IAudioSystemEffects::IID {
        EFFECTS
    } else {
        unsafe { *out = std::ptr::null_mut() };
        return E_NOINTERFACE;
    };
    let p = slot(a, n);
    // AddRef through the interface handed out: the wrapper for IUnknown, the
    // outer object for the rest.
    unsafe {
        let vtbl = &**(p as *const *const IUnknown_Vtbl);
        (vtbl.AddRef)(p);
        *out = p;
    }
    S_OK
}

unsafe extern "system" fn inner_add_ref(ptr: *mut c_void) -> u32 {
    unsafe { this::<UNKNOWN>(ptr) }.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn inner_release(ptr: *mut c_void) -> u32 {
    let a = unsafe { this::<UNKNOWN>(ptr) };
    let left = a.refs.fetch_sub(1, Ordering::Release) - 1;
    if left == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(unsafe { Box::from_raw(a as *const Aggregate as *mut Aggregate) });
    }
    left
}

// ---- delegating IUnknown on every APO interface: all to the outer object ----

unsafe extern "system" fn outer_query<const SLOT: usize>(ptr: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let a = unsafe { this::<SLOT>(ptr) };
    unsafe { (outer_vtbl(a).QueryInterface)(a.outer, iid, out) }
}

unsafe extern "system" fn outer_add_ref<const SLOT: usize>(ptr: *mut c_void) -> u32 {
    let a = unsafe { this::<SLOT>(ptr) };
    unsafe { (outer_vtbl(a).AddRef)(a.outer) }
}

unsafe extern "system" fn outer_release<const SLOT: usize>(ptr: *mut c_void) -> u32 {
    let a = unsafe { this::<SLOT>(ptr) };
    unsafe { (outer_vtbl(a).Release)(a.outer) }
}

const fn delegating<const SLOT: usize>() -> IUnknown_Vtbl {
    IUnknown_Vtbl { QueryInterface: outer_query::<SLOT>, AddRef: outer_add_ref::<SLOT>, Release: outer_release::<SLOT> }
}

// ---- forwarders: the rest of each interface, straight to the inner object ----

macro_rules! forward {
    ($name:ident, $slot:ident, $field:ident, $method:ident, ($($arg:ident: $ty:ty),*) $(-> $ret:ty)?) => {
        unsafe extern "system" fn $name(ptr: *mut c_void $(, $arg: $ty)*) $(-> $ret)? {
            let a = unsafe { this::<$slot>(ptr) };
            unsafe { (a.$field.vtable().$method)(a.$field.as_raw() $(, $arg)*) }
        }
    };
}

forward!(reset, APO, inner_apo, Reset, () -> HRESULT);
forward!(get_latency, APO, inner_apo, GetLatency, (latency: *mut i64) -> HRESULT);
forward!(get_registration_properties, APO, inner_apo, GetRegistrationProperties, (props: *mut *mut APO_REG_PROPERTIES) -> HRESULT);
forward!(initialize, APO, inner_apo, Initialize, (size: u32, data: *const u8) -> HRESULT);
forward!(is_input_format_supported, APO, inner_apo, IsInputFormatSupported, (opposite: *mut c_void, requested: *mut c_void, supported: *mut *mut c_void) -> HRESULT);
forward!(is_output_format_supported, APO, inner_apo, IsOutputFormatSupported, (opposite: *mut c_void, requested: *mut c_void, supported: *mut *mut c_void) -> HRESULT);
forward!(get_input_channel_count, APO, inner_apo, GetInputChannelCount, (count: *mut u32) -> HRESULT);
forward!(apo_process, RT, inner_rt, APOProcess, (n_in: u32, inputs: *const *const APO_CONNECTION_PROPERTY, n_out: u32, outputs: *mut *mut APO_CONNECTION_PROPERTY));
forward!(calc_input_frames, RT, inner_rt, CalcInputFrames, (frames: u32) -> u32);
forward!(calc_output_frames, RT, inner_rt, CalcOutputFrames, (frames: u32) -> u32);
forward!(lock_for_process, CONFIG, inner_config, LockForProcess, (n_in: u32, inputs: *const *const APO_CONNECTION_DESCRIPTOR, n_out: u32, outputs: *const *const APO_CONNECTION_DESCRIPTOR) -> HRESULT);
forward!(unlock_for_process, CONFIG, inner_config, UnlockForProcess, () -> HRESULT);

static UNKNOWN_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: inner_query, AddRef: inner_add_ref, Release: inner_release };

static APO_VTBL: IAudioProcessingObject_Vtbl = IAudioProcessingObject_Vtbl {
    base__: delegating::<APO>(),
    Reset: reset,
    GetLatency: get_latency,
    GetRegistrationProperties: get_registration_properties,
    Initialize: initialize,
    IsInputFormatSupported: is_input_format_supported,
    IsOutputFormatSupported: is_output_format_supported,
    GetInputChannelCount: get_input_channel_count,
};

static RT_VTBL: IAudioProcessingObjectRT_Vtbl = IAudioProcessingObjectRT_Vtbl {
    base__: delegating::<RT>(),
    APOProcess: apo_process,
    CalcInputFrames: calc_input_frames,
    CalcOutputFrames: calc_output_frames,
};

static CONFIG_VTBL: IAudioProcessingObjectConfiguration_Vtbl = IAudioProcessingObjectConfiguration_Vtbl {
    base__: delegating::<CONFIG>(),
    LockForProcess: lock_for_process,
    UnlockForProcess: unlock_for_process,
};

static EFFECTS_VTBL: IAudioSystemEffects_Vtbl = IAudioSystemEffects_Vtbl { base__: delegating::<EFFECTS>() };

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    /// A stand-in for audiodg's outer object: answers IUnknown itself, asks
    /// the aggregated APO for everything else, and counts its references.
    #[repr(C)]
    struct Outer {
        vtbl: *const IUnknown_Vtbl,
        refs: AtomicI32,
        queries: AtomicI32,
        inner: *mut c_void,
    }

    unsafe extern "system" fn o_query(ptr: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
        let o = unsafe { &*(ptr as *const Outer) };
        o.queries.fetch_add(1, Ordering::Relaxed);
        if unsafe { *iid } == IUnknown::IID {
            o.refs.fetch_add(1, Ordering::Relaxed);
            unsafe { *out = ptr };
            return S_OK;
        }
        unsafe { inner_query(o.inner, iid, out) }
    }
    unsafe extern "system" fn o_add_ref(ptr: *mut c_void) -> u32 {
        (unsafe { &*(ptr as *const Outer) }.refs.fetch_add(1, Ordering::Relaxed) + 1) as u32
    }
    unsafe extern "system" fn o_release(ptr: *mut c_void) -> u32 {
        (unsafe { &*(ptr as *const Outer) }.refs.fetch_sub(1, Ordering::Relaxed) - 1) as u32
    }
    static OUTER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: o_query, AddRef: o_add_ref, Release: o_release };

    #[test]
    fn works_aggregated_through_the_class_factory() {
        let mut outer = Box::new(Outer { vtbl: &OUTER_VTBL, refs: AtomicI32::new(1), queries: AtomicI32::new(0), inner: std::ptr::null_mut() });
        let outer_ptr = &mut *outer as *mut Outer as *mut c_void;

        // The factory accepts aggregation only for IUnknown.
        let factory: windows::Win32::System::Com::IClassFactory = crate::Factory.into();
        let outer_unknown: IUnknown = unsafe { IUnknown::from_raw_borrowed(&outer_ptr) }.unwrap().clone();
        let refused: windows::core::Result<IAudioProcessingObject> = unsafe { factory.CreateInstance(&outer_unknown) };
        assert!(refused.is_err());
        let inner: IUnknown = unsafe { factory.CreateInstance(&outer_unknown) }.unwrap();
        outer.inner = inner.as_raw();
        let base = outer.refs.load(Ordering::Relaxed);

        // Every APO interface is reachable through the outer object and works.
        let apo: IAudioProcessingObject = outer_unknown.cast().unwrap();
        assert_eq!(unsafe { apo.GetLatency() }.unwrap(), crate::LATENCY_HNS);
        let rt: IAudioProcessingObjectRT = apo.cast().unwrap();
        assert_eq!(unsafe { rt.CalcInputFrames(480) }, 480);
        let _config: IAudioProcessingObjectConfiguration = rt.cast().unwrap();
        let _effects: IAudioSystemEffects = apo.cast().unwrap();
        assert!(apo.cast::<IAudioSystemEffects2>().is_err());

        // Those interfaces count against the outer object, and a query on
        // one of them goes to the outer object too.
        assert!(outer.refs.load(Ordering::Relaxed) > base);
        let before = outer.queries.load(Ordering::Relaxed);
        let _again: IAudioProcessingObjectRT = apo.cast().unwrap();
        assert!(outer.queries.load(Ordering::Relaxed) > before);
        drop((apo, rt, _config, _effects, _again));
        assert_eq!(outer.refs.load(Ordering::Relaxed), base);
        drop(inner);
        drop(outer_unknown);
    }
}
