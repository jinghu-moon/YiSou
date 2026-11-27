use std::cell::RefCell;
use windows::{
    System::DispatcherQueueController,
    Win32::System::WinRT::{
        CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
        DispatcherQueueOptions, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
    },
    core::Result,
};

struct Queue {
    _controller: DispatcherQueueController,
    _apartment: Apartment,
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

thread_local! {
    static QUEUE: RefCell<Option<Queue>> = const { RefCell::new(None) };
}

pub(super) fn prepare() -> Result<()> {
    QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.is_none() {
            unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
            let apartment = Apartment;
            let controller = unsafe {
                CreateDispatcherQueueController(DispatcherQueueOptions {
                    dwSize: size_of::<DispatcherQueueOptions>() as u32,
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_NONE,
                })?
            };
            *queue = Some(Queue {
                _controller: controller,
                _apartment: apartment,
            });
        }
        Ok(())
    })
}
