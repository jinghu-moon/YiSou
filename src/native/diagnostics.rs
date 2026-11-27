use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    io::Write,
    time::Instant,
};
use windows_sys::Win32::System::{
    ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
    Threading::{GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources},
};

#[derive(Default)]
pub(super) struct MemoryTrace(Option<RefCell<TraceFile>>);

struct TraceFile {
    file: File,
    start: Instant,
}

impl MemoryTrace {
    pub(super) fn open() -> Self {
        let Some(path) = std::env::var_os("YISOU_MEMORY_TRACE").filter(|p| !p.is_empty()) else {
            return Self::default();
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return Self::default();
        };
        if file.metadata().is_ok_and(|m| m.len() == 0)
            && writeln!(
                file,
                "pid,elapsed_us,event,working_set_bytes,private_bytes,peak_working_set_bytes,gdi_objects,user_objects"
            )
            .is_err()
        {
            return Self::default();
        }
        Self(Some(RefCell::new(TraceFile {
            file,
            start: Instant::now(),
        })))
    }

    pub(super) fn sample(&self, event: &str) {
        let Some(trace) = &self.0 else { return };
        let mut trace = trace.borrow_mut();
        let elapsed = trace.start.elapsed().as_micros();
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let process = unsafe { GetCurrentProcess() };
        if unsafe {
            K32GetProcessMemoryInfo(
                process,
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                counters.cb,
            )
        } == 0
        {
            return;
        }
        let gdi = unsafe { GetGuiResources(process, GR_GDIOBJECTS) };
        let user = unsafe { GetGuiResources(process, GR_USEROBJECTS) };
        let _ = writeln!(
            trace.file,
            "{},{elapsed},{event},{},{},{},{gdi},{user}",
            std::process::id(),
            counters.WorkingSetSize,
            counters.PrivateUsage,
            counters.PeakWorkingSetSize,
        );
    }
}

pub(super) mod ocr {
    use std::{
        cell::RefCell,
        fs::OpenOptions,
        io::{BufWriter, Write},
        time::Instant,
    };

    thread_local! {
        // Buffer samples to avoid a file write for every mouse or paint event.
        static TRACE: RefCell<Option<BufWriter<std::fs::File>>> = RefCell::new(open());
    }

    fn open() -> Option<BufWriter<std::fs::File>> {
        let path = std::env::var_os("YISOU_OCR_TRACE").filter(|path| !path.is_empty())?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        let empty = file.metadata().ok()?.len() == 0;
        let mut trace = BufWriter::new(file);
        if empty {
            writeln!(trace, "event,duration_us").ok()?;
        }
        Some(trace)
    }

    pub fn start() -> Option<Instant> {
        TRACE.with_borrow(|trace| trace.as_ref().map(|_| Instant::now()))
    }

    pub fn record(event: &'static str, start: Option<Instant>) {
        if let Some(start) = start {
            let elapsed = start.elapsed().as_micros();
            TRACE.with_borrow_mut(|trace| {
                if let Some(trace) = trace {
                    let _ = writeln!(trace, "{event},{elapsed}");
                }
            });
        }
    }

    pub fn span(event: &'static str) -> impl Drop {
        struct Span(&'static str, Option<Instant>);

        impl Drop for Span {
            fn drop(&mut self) {
                record(self.0, self.1);
            }
        }

        Span(event, start())
    }

    pub fn flush() {
        TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                let _ = trace.flush();
            }
        });
    }
}
