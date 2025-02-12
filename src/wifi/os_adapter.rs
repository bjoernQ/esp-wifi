use embedded_hal::prelude::_embedded_hal_blocking_rng_Read;
#[cfg_attr(feature = "esp32c3", path = "os_adapter_esp32c3.rs")]
#[cfg_attr(feature = "esp32", path = "os_adapter_esp32.rs")]
pub(crate) mod os_adapter_chip_specific;

use esp_alloc::{calloc, memory_fence};
use log::{debug, trace};

use crate::{
    binary::include::*,
    compat::{
        common::{
            create_recursive_mutex, create_wifi_queue, lock_mutex, receive_queued, sem_create,
            sem_delete, sem_give, sem_take, send_queued, syslog, thread_sem_get, unlock_mutex,
            StrBuf,
        },
        timer_compat::{
            compat_esp_timer_create, compat_timer_arm, compat_timer_arm_us, compat_timer_disarm,
            compat_timer_done, compat_timer_setfn,
        },
        work_queue::queue_work,
    },
    wifi::RANDOM_GENERATOR,
};

pub static mut WIFI_STATE: i32 = -1;

pub fn is_connected() -> bool {
    unsafe { WIFI_STATE == wifi_event_t_WIFI_EVENT_STA_CONNECTED as i32 }
}

#[derive(Debug, Clone, Copy)]
pub enum WifiState {
    WifiReady,
    StaStart,
    StaStop,
    StaConnected,
    StaDisconnected,
    Invalid,
}

#[allow(non_upper_case_globals)]
pub fn get_wifi_state() -> WifiState {
    match unsafe { WIFI_STATE as u32 } {
        wifi_event_t_WIFI_EVENT_WIFI_READY => WifiState::WifiReady,
        wifi_event_t_WIFI_EVENT_STA_START => WifiState::StaStart,
        wifi_event_t_WIFI_EVENT_STA_STOP => WifiState::StaStop,
        wifi_event_t_WIFI_EVENT_STA_CONNECTED => WifiState::StaConnected,
        wifi_event_t_WIFI_EVENT_STA_DISCONNECTED => WifiState::StaDisconnected,
        _ => WifiState::Invalid,
    }
}

/****************************************************************************
 * Name: esp_event_send_internal
 *
 * Description:
 *   Post event message to queue
 *
 * Input Parameters:
 *   event_base      - Event set name
 *   event_id        - Event ID
 *   event_data      - Event private data
 *   event_data_size - Event data size
 *   ticks_to_wait   - Waiting system ticks
 *
 * Returned Value:
 *   Task maximum priority
 *
 ****************************************************************************/
pub unsafe extern "C" fn esp_event_send_internal(
    event_base: esp_event_base_t,
    event_id: i32,
    event_data: *mut crate::binary::c_types::c_void,
    event_data_size: size_t,
    ticks_to_wait: TickType_t,
) -> esp_err_t {
    trace!(
        "esp_event_send_internal {:?} {} {:p} {} {:?}",
        event_base,
        event_id,
        event_data,
        event_data_size,
        ticks_to_wait
    );

    // probably also need to look at event_base
    #[allow(non_upper_case_globals)]
    let take_state = match event_id as u32 {
        wifi_event_t_WIFI_EVENT_WIFI_READY => true,
        wifi_event_t_WIFI_EVENT_STA_START => true,
        wifi_event_t_WIFI_EVENT_STA_STOP => true,
        wifi_event_t_WIFI_EVENT_STA_CONNECTED => true,
        wifi_event_t_WIFI_EVENT_STA_DISCONNECTED => true,
        _ => false,
    };

    if take_state {
        WIFI_STATE = event_id;
    }

    memory_fence();

    0
}

/****************************************************************************
 * Name: wifi_env_is_chip
 *
 * Description:
 *   Config chip environment
 *
 * Returned Value:
 *   True if on chip or false if on FPGA.
 *
 ****************************************************************************/
pub unsafe extern "C" fn env_is_chip() -> bool {
    true
}

/****************************************************************************
 * Name: wifi_set_intr
 *
 * Description:
 *   Do nothing
 *
 * Input Parameters:
 *     cpu_no      - The CPU which the interrupt number belongs.
 *     intr_source - The interrupt hardware source number.
 *     intr_num    - The interrupt number CPU.
 *     intr_prio   - The interrupt priority.
 *
 * Returned Value:
 *     None
 *
 ****************************************************************************/
pub unsafe extern "C" fn set_intr(cpu_no: i32, intr_source: u32, intr_num: u32, intr_prio: i32) {
    trace!(
        "set_intr {} {} {} {}",
        cpu_no,
        intr_source,
        intr_num,
        intr_prio
    );

    crate::wifi::os_adapter::os_adapter_chip_specific::set_intr(
        cpu_no,
        intr_source,
        intr_num,
        intr_prio,
    );
}

/****************************************************************************
 * Name: wifi_clear_intr
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn clear_intr(_intr_source: u32, _intr_num: u32) {
    // original code does nothing here
    debug!("clear_intr called {} {}", _intr_source, _intr_num);
}

pub static mut ISR_INTERRUPT_1: (
    *mut crate::binary::c_types::c_void,
    *mut crate::binary::c_types::c_void,
) = (core::ptr::null_mut(), core::ptr::null_mut());

/****************************************************************************
 * Name: esp_set_isr
 *
 * Description:
 *   Register interrupt function
 *
 * Input Parameters:
 *   n   - Interrupt ID
 *   f   - Interrupt function
 *   arg - Function private data
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn set_isr(
    n: i32,
    f: *mut crate::binary::c_types::c_void,
    arg: *mut crate::binary::c_types::c_void,
) {
    trace!("set_isr - interrupt {} function {:p} arg {:p}", n, f, arg);

    match n {
        0 => {
            ISR_INTERRUPT_1 = (f, arg);
        }
        1 => {
            ISR_INTERRUPT_1 = (f, arg);
        }
        _ => panic!("set_isr - unsupported interrupt number {}", n),
    }
}

/****************************************************************************
 * Name: esp32c3_ints_on
 *
 * Description:
 *   Enable Wi-Fi interrupt
 *
 * Input Parameters:
 *   mask - No mean
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn ints_on(mask: u32) {
    trace!("ints_on {:x}", mask);

    crate::wifi::os_adapter::os_adapter_chip_specific::chip_ints_on(mask);
}

/****************************************************************************
 * Name: esp32c3_ints_off
 *
 * Description:
 *   Disable Wi-Fi interrupt
 *
 * Input Parameters:
 *   mask - No mean
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn ints_off(mask: u32) {
    trace!("ints_off - not implemented - {:x}", mask);
}

/****************************************************************************
 * Name: wifi_is_from_isr
 *
 * Description:
 *   Check current is in interrupt
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   true if in interrupt or false if not
 *
 ****************************************************************************/
pub unsafe extern "C" fn is_from_isr() -> bool {
    true
}

/****************************************************************************
 * Name: esp_spin_lock_create
 *
 * Description:
 *   Create spin lock in SMP mode
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Spin lock data pointer
 *
 ****************************************************************************/
static mut FAKE_SPIN_LOCK: u8 = 1;
pub unsafe extern "C" fn spin_lock_create() -> *mut crate::binary::c_types::c_void {
    // original: return (void *)1;
    let ptr = &mut FAKE_SPIN_LOCK as *mut _ as *mut crate::binary::c_types::c_void;
    trace!("spin_lock_create {:p}", ptr);
    ptr
}

/****************************************************************************
 * Name: esp_spin_lock_delete
 *
 * Description:
 *   Delete spin lock
 *
 * Input Parameters:
 *   lock - Spin lock data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn spin_lock_delete(lock: *mut crate::binary::c_types::c_void) {
    // original: DEBUGASSERT((int)lock == 1);
    trace!("spin_lock_delete {:p}", lock);
}

/****************************************************************************
 * Name: esp_wifi_int_disable
 *
 * Description:
 *   Enter critical section by disabling interrupts and taking the spin lock
 *   if in SMP mode.
 *
 * Input Parameters:
 *   wifi_int_mux - Spin lock data pointer
 *
 * Returned Value:
 *   CPU PS value.
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_int_disable(
    wifi_int_mux: *mut crate::binary::c_types::c_void,
) -> u32 {
    crate::wifi::os_adapter::os_adapter_chip_specific::wifi_int_disable(wifi_int_mux)
}

/****************************************************************************
 * Name: esp_wifi_int_restore
 *
 * Description:
 *   Exit from critical section by enabling interrupts and releasing the spin
 *   lock if in SMP mode.
 *
 * Input Parameters:
 *   wifi_int_mux - Spin lock data pointer
 *   tmp          - CPU PS value.
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_int_restore(
    wifi_int_mux: *mut crate::binary::c_types::c_void,
    tmp: u32,
) {
    crate::wifi::os_adapter::os_adapter_chip_specific::wifi_int_restore(wifi_int_mux, tmp)
}

/****************************************************************************
 * Name: esp_task_yield_from_isr
 *
 * Description:
 *   Do nothing in NuttX
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_yield_from_isr() {
    // original: /* Do nothing */
    trace!("task_yield_from_isr")
}

/****************************************************************************
 * Name: esp_semphr_create
 *
 * Description:
 *   Create and initialize semaphore
 *
 * Input Parameters:
 *   max  - No mean
 *   init - semaphore initialization value
 *
 * Returned Value:
 *   Semaphore data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut crate::binary::c_types::c_void {
    trace!("semphr_create - max {} init {}", max, init);
    sem_create(max, init)
}

/****************************************************************************
 * Name: esp_semphr_delete
 *
 * Description:
 *   Delete semaphore
 *
 * Input Parameters:
 *   semphr - Semaphore data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn semphr_delete(semphr: *mut crate::binary::c_types::c_void) {
    trace!("semphr_delete {:p}", semphr);
    sem_delete(semphr);
}

/****************************************************************************
 * Name: esp_semphr_take
 *
 * Description:
 *   Wait semaphore within a certain period of time
 *
 * Input Parameters:
 *   semphr - Semaphore data pointer
 *   ticks  - Wait system ticks
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn semphr_take(
    semphr: *mut crate::binary::c_types::c_void,
    tick: u32,
) -> i32 {
    sem_take(semphr, tick)
}

/****************************************************************************
 * Name: esp_semphr_give
 *
 * Description:
 *   Post semaphore
 *
 * Input Parameters:
 *   semphr - Semaphore data pointer
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn semphr_give(semphr: *mut crate::binary::c_types::c_void) -> i32 {
    sem_give(semphr)
}

/****************************************************************************
 * Name: esp_thread_semphr_get
 *
 * Description:
 *   Get thread self's semaphore
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Semaphore data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_thread_semphr_get() -> *mut crate::binary::c_types::c_void {
    thread_sem_get()
}

/****************************************************************************
 * Name: esp_mutex_create
 *
 * Description:
 *   Create mutex
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Mutex data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn mutex_create() -> *mut crate::binary::c_types::c_void {
    todo!("mutex_create")
}

/****************************************************************************
 * Name: esp_recursive_mutex_create
 *
 * Description:
 *   Create recursive mutex
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Recursive mutex data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn recursive_mutex_create() -> *mut crate::binary::c_types::c_void {
    create_recursive_mutex()
}

/****************************************************************************
 * Name: esp_mutex_delete
 *
 * Description:
 *   Delete mutex
 *
 * Input Parameters:
 *   mutex_data - mutex data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn mutex_delete(_mutex: *mut crate::binary::c_types::c_void) {
    todo!("mutex_delete")
}

/****************************************************************************
 * Name: esp_mutex_lock
 *
 * Description:
 *   Lock mutex
 *
 * Input Parameters:
 *   mutex_data - mutex data pointer
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn mutex_lock(mutex: *mut crate::binary::c_types::c_void) -> i32 {
    lock_mutex(mutex)
}

/****************************************************************************
 * Name: esp_mutex_unlock
 *
 * Description:
 *   Unlock mutex
 *
 * Input Parameters:
 *   mutex_data - mutex data pointer
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn mutex_unlock(mutex: *mut crate::binary::c_types::c_void) -> i32 {
    unlock_mutex(mutex)
}

/****************************************************************************
 * Name: esp_queue_create
 *
 * Description:
 *   Create message queue
 *
 * Input Parameters:
 *   queue_len - queue message number
 *   item_size - message size
 *
 * Returned Value:
 *   Message queue data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_create(
    _queue_len: u32,
    _item_size: u32,
) -> *mut crate::binary::c_types::c_void {
    todo!("queue_create")
}

/****************************************************************************
 * Name: esp_queue_delete
 *
 * Description:
 *   Delete message queue
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_delete(_queue: *mut crate::binary::c_types::c_void) {
    todo!("queue_delete")
}

/****************************************************************************
 * Name: esp_queue_send
 *
 * Description:
 *   Send message of low priority to queue within a certain period of time
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *   item  - Message data pointer
 *   ticks - Wait ticks
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_send(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    send_queued(queue, item, block_time_tick)
}

/****************************************************************************
 * Name: esp_queue_send_from_isr
 *
 * Description:
 *   Send message of low priority to queue in ISR within
 *   a certain period of time
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *   item  - Message data pointer
 *   hptw  - No mean
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_send_from_isr(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    _hptw: *mut crate::binary::c_types::c_void,
) -> i32 {
    trace!("queue_send_from_isr");
    queue_send(queue, item, 1000)
}

/****************************************************************************
 * Name: esp_queue_send_to_back
 *
 * Description:
 *   Send message of low priority to queue within a certain period of time
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *   item  - Message data pointer
 *   ticks - Wait ticks
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_send_to_back(
    _queue: *mut crate::binary::c_types::c_void,
    _item: *mut crate::binary::c_types::c_void,
    _block_time_tick: u32,
) -> i32 {
    todo!("queue_send_to_back")
}

/****************************************************************************
 * Name: esp_queue_send_from_to_front
 *
 * Description:
 *   Send message of high priority to queue within a certain period of time
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *   item  - Message data pointer
 *   ticks - Wait ticks
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_send_to_front(
    _queue: *mut crate::binary::c_types::c_void,
    _item: *mut crate::binary::c_types::c_void,
    _block_time_tick: u32,
) -> i32 {
    todo!("queue_send_to_front")
}

/****************************************************************************
 * Name: esp_queue_recv
 *
 * Description:
 *   Receive message from queue within a certain period of time
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *   item  - Message data pointer
 *   ticks - Wait ticks
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_recv(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    receive_queued(queue, item, block_time_tick)
}

/****************************************************************************
 * Name: esp_queue_msg_waiting
 *
 * Description:
 *   Get message number in the message queue
 *
 * Input Parameters:
 *   queue - Message queue data pointer
 *
 * Returned Value:
 *   Message number
 *
 ****************************************************************************/
pub unsafe extern "C" fn queue_msg_waiting(_queue: *mut crate::binary::c_types::c_void) -> u32 {
    todo!("queue_msg_waiting")
}

/****************************************************************************
 * Name: esp_event_group_create
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_group_create() -> *mut crate::binary::c_types::c_void {
    todo!("event_group_create")
}

/****************************************************************************
 * Name: esp_event_group_delete
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_group_delete(_event: *mut crate::binary::c_types::c_void) {
    todo!("event_group_delete")
}

/****************************************************************************
 * Name: esp_event_group_set_bits
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_group_set_bits(
    _event: *mut crate::binary::c_types::c_void,
    _bits: u32,
) -> u32 {
    todo!("event_group_set_bits")
}

/****************************************************************************
 * Name: esp_event_group_clear_bits
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_group_clear_bits(
    _event: *mut crate::binary::c_types::c_void,
    _bits: u32,
) -> u32 {
    todo!("event_group_clear_bits")
}

/****************************************************************************
 * Name: esp_event_group_wait_bits
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_group_wait_bits(
    _event: *mut crate::binary::c_types::c_void,
    _bits_to_wait_for: u32,
    _clear_on_exit: crate::binary::c_types::c_int,
    _wait_for_all_bits: crate::binary::c_types::c_int,
    _block_time_tick: u32,
) -> u32 {
    todo!("event_group_wait_bits")
}

/****************************************************************************
 * Name: esp_task_create_pinned_to_core
 *
 * Description:
 *   Create task and bind it to target CPU, the task will run when it
 *   is created
 *
 * Input Parameters:
 *   entry       - Task entry
 *   name        - Task name
 *   stack_depth - Task stack size
 *   param       - Task private data
 *   prio        - Task priority
 *   task_handle - Task handle pointer which is used to pause, resume
 *                 and delete the task
 *   core_id     - CPU which the task runs in
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_create_pinned_to_core(
    task_func: *mut crate::binary::c_types::c_void,
    name: *const crate::binary::c_types::c_char,
    stack_depth: u32,
    param: *mut crate::binary::c_types::c_void,
    prio: u32,
    task_handle: *mut crate::binary::c_types::c_void,
    core_id: u32,
) -> i32 {
    trace!("task_create_pinned_to_core task_func {:p} name {} stack_depth {} param {:p} prio {}, task_handle {:p} core_id {}",
        task_func,
        StrBuf::from(name).as_str_ref(),
        stack_depth,
        param,
        prio,
        task_handle,
        core_id
    );

    *(task_handle as *mut usize) = 0; // we will run it in task 0

    queue_work(
        task_func,
        name,
        stack_depth,
        param,
        prio,
        task_handle,
        core_id,
    );
    1
}

/****************************************************************************
 * Name: esp_task_create
 *
 * Description:
 *   Create task and the task will run when it is created
 *
 * Input Parameters:
 *   entry       - Task entry
 *   name        - Task name
 *   stack_depth - Task stack size
 *   param       - Task private data
 *   prio        - Task priority
 *   task_handle - Task handle pointer which is used to pause, resume
 *                 and delete the task
 *
 * Returned Value:
 *   True if success or false if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_create(
    _task_func: *mut crate::binary::c_types::c_void,
    _name: *const crate::binary::c_types::c_char,
    _stack_depth: u32,
    _param: *mut crate::binary::c_types::c_void,
    _prio: u32,
    _task_handle: *mut crate::binary::c_types::c_void,
) -> i32 {
    todo!("task_create");
}

/****************************************************************************
 * Name: esp_task_delete
 *
 * Description:
 *   Delete the target task
 *
 * Input Parameters:
 *   task_handle - Task handle pointer which is used to pause, resume
 *                 and delete the task
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_delete(_task_handle: *mut crate::binary::c_types::c_void) {
    todo!("task_delete")
}

/****************************************************************************
 * Name: esp_task_delay
 *
 * Description:
 *   Current task wait for some ticks
 *
 * Input Parameters:
 *   tick - Waiting ticks
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_delay(tick: u32) {
    trace!("task_delay tick {}", tick);
    let mut now = crate::timer::get_systimer_count();
    let timeout = now + tick as u64;
    loop {
        if now > timeout {
            break;
        }
        now = crate::timer::get_systimer_count();
    }
}

/****************************************************************************
 * Name: esp_task_ms_to_tick
 *
 * Description:
 *   Transform from millim seconds to system ticks
 *
 * Input Parameters:
 *   ms - Millim seconds
 *
 * Returned Value:
 *   System ticks
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_ms_to_tick(ms: u32) -> i32 {
    trace!("task_ms_to_tick ms {}", ms);
    (ms as u64 * crate::timer::TICKS_PER_SECOND / 1000) as i32
}

/****************************************************************************
 * Name: esp_task_get_current_task
 *
 * Description:
 *   Transform from millim seconds to system ticks
 *
 * Input Parameters:
 *   ms - Millim seconds
 *
 * Returned Value:
 *   System ticks
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_get_current_task() -> *mut crate::binary::c_types::c_void {
    let res = crate::preempt::preempt::current_task() as *mut crate::binary::c_types::c_void;
    trace!("task get current task - return {:p}", res);

    res
}

/****************************************************************************
 * Name: esp_task_get_max_priority
 *
 * Description:
 *   Get OS task maximum priority
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Task maximum priority
 *
 ****************************************************************************/
pub unsafe extern "C" fn task_get_max_priority() -> i32 {
    trace!("task_get_max_priority");
    255
}

/****************************************************************************
 * Name: esp_malloc
 *
 * Description:
 *   Allocate a block of memory
 *
 * Input Parameters:
 *   size - memory size
 *
 * Returned Value:
 *   Memory pointer
 *
 ****************************************************************************/
#[no_mangle]
pub unsafe extern "C" fn malloc(
    size: crate::binary::c_types::c_uint,
) -> *mut crate::binary::c_types::c_void {
    esp_alloc::malloc(size as u32) as *mut crate::binary::c_types::c_void
}

/****************************************************************************
 * Name: esp_free
 *
 * Description:
 *   Free a block of memory
 *
 * Input Parameters:
 *   ptr - memory block
 *
 * Returned Value:
 *   No
 *
 ****************************************************************************/
#[no_mangle]
pub unsafe extern "C" fn free(p: *mut crate::binary::c_types::c_void) {
    esp_alloc::free(p as *const _ as *const u8);
}

/****************************************************************************
 * Name: esp_event_post
 *
 * Description:
 *   Active work queue and let the work to process the cached event
 *
 * Input Parameters:
 *   event_base      - Event set name
 *   event_id        - Event ID
 *   event_data      - Event private data
 *   event_data_size - Event data size
 *   ticks           - Waiting system ticks
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn event_post(
    _event_base: *const crate::binary::c_types::c_char,
    _event_id: i32,
    _event_data: *mut crate::binary::c_types::c_void,
    _event_data_size: size_t,
    _ticks_to_wait: u32,
) -> i32 {
    todo!("event_post")
}

/****************************************************************************
 * Name: esp_get_free_heap_size
 *
 * Description:
 *   Get free heap size by byte
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Free heap size
 *
 ****************************************************************************/
pub unsafe extern "C" fn get_free_heap_size() -> u32 {
    todo!("get_free_heap_size")
}

/****************************************************************************
 * Name: esp_rand
 *
 * Description:
 *   Get random data of type uint32_t
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   Random data
 *
 ****************************************************************************/
pub unsafe extern "C" fn rand() -> u32 {
    todo!("rand")
}

/****************************************************************************
 * Name: esp_dport_access_stall_other_cpu_start
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn dport_access_stall_other_cpu_start_wrap() {
    trace!("dport_access_stall_other_cpu_start_wrap")
}

/****************************************************************************
 * Name: esp_dport_access_stall_other_cpu_end
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn dport_access_stall_other_cpu_end_wrap() {
    trace!("dport_access_stall_other_cpu_end_wrap")
}
/****************************************************************************
 * Name: wifi_apb80m_request
 *
 * Description:
 *   Take Wi-Fi lock in auto-sleep
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_apb80m_request() {
    trace!("wifi_apb80m_request - no-op")
}
/****************************************************************************
 * Name: wifi_apb80m_release
 *
 * Description:
 *   Release Wi-Fi lock in auto-sleep
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_apb80m_release() {
    trace!("wifi_apb80m_release - no-op")
}

/****************************************************************************
 * Name: esp32c3_phy_disable
 *
 * Description:
 *   Deinitialize PHY hardware
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn phy_disable() {
    trace!("phy_disable")
}

/****************************************************************************
 * Name: esp32c3_phy_enable
 *
 * Description:
 *   Initialize PHY hardware
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn phy_enable() {
    // quite some code needed here
    trace!("phy_enable - not fully implemented");

    crate::wifi::os_adapter::os_adapter_chip_specific::phy_enable();
}

/****************************************************************************
 * Name: wifi_phy_update_country_info
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn phy_update_country_info(
    country: *const crate::binary::c_types::c_char,
) -> crate::binary::c_types::c_int {
    // not implemented in original code
    trace!("phy_update_country_info {}", *country as char);
    -1
}

/****************************************************************************
 * Name: esp_wifi_read_mac
 *
 * Description:
 *   Read MAC address from efuse
 *
 * Input Parameters:
 *   mac  - MAC address buffer pointer
 *   type - MAC address type
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn read_mac(mac: *mut u8, type_: u32) -> crate::binary::c_types::c_int {
    trace!("read_mac {:p} {}", mac, type_);

    crate::wifi::os_adapter::os_adapter_chip_specific::read_mac(mac, type_)
}

/****************************************************************************
 * Name: ets_timer_arm
 *
 * Description:
 *   Set timer timeout period and repeat flag
 *
 * Input Parameters:
 *   ptimer - timer data pointer
 *   ms     - millim seconds
 *   repeat - true: run cycle, false: run once
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn timer_arm(
    ptimer: *mut crate::binary::c_types::c_void,
    tmout: u32,
    repeat: bool,
) {
    compat_timer_arm(ptimer, tmout, repeat);
}

/****************************************************************************
 * Name: ets_timer_disarm
 *
 * Description:
 *   Disable timer
 *
 * Input Parameters:
 *   ptimer - timer data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn timer_disarm(ptimer: *mut crate::binary::c_types::c_void) {
    compat_timer_disarm(ptimer);
}

/****************************************************************************
 * Name: ets_timer_done
 *
 * Description:
 *   Disable and free timer
 *
 * Input Parameters:
 *   ptimer - timer data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn timer_done(ptimer: *mut crate::binary::c_types::c_void) {
    compat_timer_done(ptimer);
}

/****************************************************************************
 * Name: ets_timer_setfn
 *
 * Description:
 *   Set timer callback function and private data
 *
 * Input Parameters:
 *   ptimer    - Timer data pointer
 *   pfunction - Callback function
 *   parg      - Callback function private data
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn timer_setfn(
    ptimer: *mut crate::binary::c_types::c_void,
    pfunction: *mut crate::binary::c_types::c_void,
    parg: *mut crate::binary::c_types::c_void,
) {
    compat_timer_setfn(ptimer, pfunction, parg);
}

/****************************************************************************
 * Name: ets_timer_arm_us
 *
 * Description:
 *   Set timer timeout period and repeat flag
 *
 * Input Parameters:
 *   ptimer - timer data pointer
 *   us     - micro seconds
 *   repeat - true: run cycle, false: run once
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn timer_arm_us(
    ptimer: *mut crate::binary::c_types::c_void,
    us: u32,
    repeat: bool,
) {
    compat_timer_arm_us(ptimer, us, repeat);
}

/****************************************************************************
 * Name: wifi_reset_mac
 *
 * Description:
 *   Reset Wi-Fi hardware MAC
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_reset_mac() {
    trace!("wifi_reset_mac - not implemented");
    // modifyreg32(SYSCON_WIFI_RST_EN_REG, 0, SYSTEM_MAC_RST);
    // modifyreg32(SYSCON_WIFI_RST_EN_REG, SYSTEM_MAC_RST, 0);
}

/****************************************************************************
 * Name: wifi_clock_enable
 *
 * Description:
 *   Enable Wi-Fi clock
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_clock_enable() {
    trace!("wifi_clock_enable");
    crate::wifi::os_adapter::os_adapter_chip_specific::wifi_clock_enable();
}

/****************************************************************************
 * Name: wifi_clock_disable
 *
 * Description:
 *   Disable Wi-Fi clock
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_clock_disable() {
    trace!("wifi_clock_disable");
    crate::wifi::os_adapter::os_adapter_chip_specific::wifi_clock_disable();
}

/****************************************************************************
 * Name: wifi_rtc_enable_iso
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_rtc_enable_iso() {
    todo!("wifi_rtc_enable_iso")
}

/****************************************************************************
 * Name: wifi_rtc_disable_iso
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_rtc_disable_iso() {
    todo!("wifi_rtc_disable_iso")
}

/****************************************************************************
 * Name: esp_timer_get_time
 *
 * Description:
 *   Get system time of type int64_t
 *
 * Input Parameters:
 *   periph - No mean
 *
 * Returned Value:
 *   System time
 *
 ****************************************************************************/
#[no_mangle]
pub unsafe extern "C" fn esp_timer_get_time() -> i64 {
    trace!("esp_timer_get_time");
    (crate::timer::get_systimer_count() / crate::timer::TICKS_PER_SECOND / 1_000) as i64
}

/****************************************************************************
 * Name: esp_nvs_set_i8
 *
 * Description:
 *   Save data of type int8_t into file system
 *
 * Input Parameters:
 *   handle - NVS handle
 *   key    - Data index
 *   value  - Stored data
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_set_i8(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _value: i8,
) -> crate::binary::c_types::c_int {
    debug!("nvs_set_i8");
    -1
}

/****************************************************************************
 * Name: esp_nvs_get_i8
 *
 * Description:
 *   Read data of type int8_t from file system
 *
 * Input Parameters:
 *   handle    - NVS handle
 *   key       - Data index
 *   out_value - Read buffer pointer
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_get_i8(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _out_value: *mut i8,
) -> crate::binary::c_types::c_int {
    todo!("nvs_get_i8")
}

/****************************************************************************
 * Name: esp_nvs_set_u8
 *
 * Description:
 *   Save data of type uint8_t into file system
 *
 * Input Parameters:
 *   handle - NVS handle
 *   key    - Data index
 *   value  - Stored data
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_set_u8(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _value: u8,
) -> crate::binary::c_types::c_int {
    todo!("nvs_set_u8")
}

/****************************************************************************
 * Name: esp_nvs_get_u8
 *
 * Description:
 *   Read data of type uint8_t from file system
 *
 * Input Parameters:
 *   handle    - NVS handle
 *   key       - Data index
 *   out_value - Read buffer pointer
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_get_u8(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _out_value: *mut u8,
) -> crate::binary::c_types::c_int {
    todo!("nvs_get_u8")
}

/****************************************************************************
 * Name: esp_nvs_set_u16
 *
 * Description:
 *   Save data of type uint16_t into file system
 *
 * Input Parameters:
 *   handle - NVS handle
 *   key    - Data index
 *   value  - Stored data
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_set_u16(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _value: u16,
) -> crate::binary::c_types::c_int {
    todo!("nvs_set_u16")
}

/****************************************************************************
 * Name: esp_nvs_get_u16
 *
 * Description:
 *   Read data of type uint16_t from file system
 *
 * Input Parameters:
 *   handle    - NVS handle
 *   key       - Data index
 *   out_value - Read buffer pointer
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_get_u16(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _out_value: *mut u16,
) -> crate::binary::c_types::c_int {
    todo!("nvs_get_u16")
}

/****************************************************************************
 * Name: esp_nvs_open
 *
 * Description:
 *   Create a file system storage data object
 *
 * Input Parameters:
 *   name       - Storage index
 *   open_mode  - Storage mode
 *   out_handle - Storage handle
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_open(
    _name: *const crate::binary::c_types::c_char,
    _open_mode: u32,
    _out_handle: *mut u32,
) -> crate::binary::c_types::c_int {
    todo!("nvs_open")
}

/****************************************************************************
 * Name: esp_nvs_close
 *
 * Description:
 *   Close storage data object and free resource
 *
 * Input Parameters:
 *   handle - NVS handle
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_close(_handle: u32) {
    todo!("nvs_close")
}

/****************************************************************************
 * Name: esp_nvs_commit
 *
 * Description:
 *   This function has no practical effect
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_commit(_handle: u32) -> crate::binary::c_types::c_int {
    todo!("nvs_commit")
}

/****************************************************************************
 * Name: esp_nvs_set_blob
 *
 * Description:
 *   Save a block of data into file system
 *
 * Input Parameters:
 *   handle - NVS handle
 *   key    - Data index
 *   value  - Stored buffer pointer
 *   length - Buffer length
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_set_blob(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _value: *const crate::binary::c_types::c_void,
    _length: size_t,
) -> crate::binary::c_types::c_int {
    todo!("nvs_set_blob")
}

/****************************************************************************
 * Name: esp_nvs_get_blob
 *
 * Description:
 *   Read a block of data from file system
 *
 * Input Parameters:
 *   handle    - NVS handle
 *   key       - Data index
 *   out_value - Read buffer pointer
 *   length    - Buffer length
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_get_blob(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
    _out_value: *mut crate::binary::c_types::c_void,
    _length: *mut size_t,
) -> crate::binary::c_types::c_int {
    todo!("nvs_get_blob")
}

/****************************************************************************
 * Name: esp_nvs_erase_key
 *
 * Description:
 *   Read a block of data from file system
 *
 * Input Parameters:
 *   handle    - NVS handle
 *   key       - Data index
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn nvs_erase_key(
    _handle: u32,
    _key: *const crate::binary::c_types::c_char,
) -> crate::binary::c_types::c_int {
    todo!("nvs_erase_key")
}

/****************************************************************************
 * Name: esp_get_random
 *
 * Description:
 *   Fill random data int given buffer of given length
 *
 * Input Parameters:
 *   buf - buffer pointer
 *   len - buffer length
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn get_random(_buf: *mut u8, _len: size_t) -> crate::binary::c_types::c_int {
    todo!("get_random")
}

/****************************************************************************
 * Name: esp_get_time
 *
 * Description:
 *   Get std C time
 *
 * Input Parameters:
 *   t - buffer to store time of type timeval
 *
 * Returned Value:
 *   0 if success or -1 if fail
 *
 ****************************************************************************/
pub unsafe extern "C" fn get_time(
    _t: *mut crate::binary::c_types::c_void,
) -> crate::binary::c_types::c_int {
    todo!("get_time")
}

/****************************************************************************
 * Name: esp_random_ulong
 ****************************************************************************/
pub unsafe extern "C" fn random() -> crate::binary::c_types::c_ulong {
    trace!("random");

    if let Some(ref mut rng) = RANDOM_GENERATOR {
        let mut buffer = [0u8; 4];
        rng.read(&mut buffer).unwrap();
        u32::from_le_bytes(buffer)
    } else {
        0
    }
}

/****************************************************************************
 * Name: esp_log_write
 *
 * Description:
 *   Output log with by format string and its arguments
 *
 * Input Parameters:
 *   level  - log level, no mean here
 *   tag    - log TAG, no mean here
 *   format - format string
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
#[allow(unreachable_code)]
pub unsafe extern "C" fn log_write(
    _level: u32,
    _tag: *const crate::binary::c_types::c_char,
    _format: *const crate::binary::c_types::c_char,
    _args: ...
) {
    #[cfg(not(feature = "wifi_logs"))]
    return;

    #[cfg(feature = "esp32c3")]
    syslog(_level, _format, _args);
}

/****************************************************************************
 * Name: esp_log_writev
 *
 * Description:
 *   Output log with by format string and its arguments
 *
 * Input Parameters:
 *   level  - log level, no mean here
 *   tag    - log TAG, no mean here
 *   format - format string
 *   args   - arguments list
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn log_writev(
    _level: u32,
    _tag: *const crate::binary::c_types::c_char,
    _format: *const crate::binary::c_types::c_char,
    _args: va_list,
) {
    #[cfg(not(feature = "wifi_logs"))]
    return;

    #[cfg(feature = "esp32")]
    #[allow(unreachable_code)]
    {
        let s = StrBuf::from(_format);
        log::info!("{}", s.as_str_ref());
    }

    #[cfg(feature = "esp32c3")]
    #[allow(unreachable_code)]
    {
        let _args = core::mem::transmute(_args);
        syslog(_level, _format, _args);
    }
}

/****************************************************************************
 * Name: esp_log_timestamp
 *
 * Description:
 *   Get system time by millim second
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   System time
 *
 ****************************************************************************/
pub unsafe extern "C" fn log_timestamp() -> u32 {
    (crate::timer::get_systimer_count() / crate::timer::TICKS_PER_SECOND / 1_000) as u32
}

/****************************************************************************
 * Name: esp_malloc_internal
 *
 * Description:
 *   Drivers allocate a block of memory
 *
 * Input Parameters:
 *   size - memory size
 *
 * Returned Value:
 *   Memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn malloc_internal(size: size_t) -> *mut crate::binary::c_types::c_void {
    esp_alloc::malloc(size as u32) as *mut crate::binary::c_types::c_void
}

/****************************************************************************
 * Name: esp_realloc_internal
 *
 * Description:
 *   Drivers allocate a block of memory by old memory block
 *
 * Input Parameters:
 *   ptr  - old memory pointer
 *   size - memory size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn realloc_internal(
    _ptr: *mut crate::binary::c_types::c_void,
    _size: size_t,
) -> *mut crate::binary::c_types::c_void {
    todo!("realloc_internal")
}

/****************************************************************************
 * Name: esp_calloc_internal
 *
 * Description:
 *   Drivers allocate some continuous blocks of memory
 *
 * Input Parameters:
 *   n    - memory block number
 *   size - memory block size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn calloc_internal(
    n: size_t,
    size: size_t,
) -> *mut crate::binary::c_types::c_void {
    calloc(n as u32, size as u32) as *mut crate::binary::c_types::c_void
}

/****************************************************************************
 * Name: esp_zalloc_internal
 *
 * Description:
 *   Drivers allocate a block of memory and clear it with 0
 *
 * Input Parameters:
 *   size - memory size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn zalloc_internal(size: size_t) -> *mut crate::binary::c_types::c_void {
    calloc(size as u32, 1u32) as *mut crate::binary::c_types::c_void
}

/****************************************************************************
 * Name: esp_wifi_malloc
 *
 * Description:
 *   Applications allocate a block of memory
 *
 * Input Parameters:
 *   size - memory size
 *
 * Returned Value:
 *   Memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_malloc(size: size_t) -> *mut crate::binary::c_types::c_void {
    malloc(size as u32)
}

/****************************************************************************
 * Name: esp_wifi_realloc
 *
 * Description:
 *   Applications allocate a block of memory by old memory block
 *
 * Input Parameters:
 *   ptr  - old memory pointer
 *   size - memory size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_realloc(
    _ptr: *mut crate::binary::c_types::c_void,
    _size: size_t,
) -> *mut crate::binary::c_types::c_void {
    todo!("wifi_realloc")
}

/****************************************************************************
 * Name: esp_wifi_calloc
 *
 * Description:
 *   Applications allocate some continuous blocks of memory
 *
 * Input Parameters:
 *   n    - memory block number
 *   size - memory block size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_calloc(
    n: size_t,
    size: size_t,
) -> *mut crate::binary::c_types::c_void {
    trace!("wifi_calloc {} {}", n, size);
    calloc(n as u32, size as u32) as *mut crate::binary::c_types::c_void
}

/****************************************************************************
 * Name: esp_wifi_zalloc
 *
 * Description:
 *   Applications allocate a block of memory and clear it with 0
 *
 * Input Parameters:
 *   size - memory size
 *
 * Returned Value:
 *   New memory pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_zalloc(size: size_t) -> *mut crate::binary::c_types::c_void {
    wifi_calloc(size, 1)
}

/****************************************************************************
 * Name: esp_wifi_create_queue
 *
 * Description:
 *   Create Wi-Fi static message queue
 *
 * Input Parameters:
 *   queue_len - queue message number
 *   item_size - message size
 *
 * Returned Value:
 *   Wi-Fi static message queue data pointer
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_create_queue(
    queue_len: crate::binary::c_types::c_int,
    item_size: crate::binary::c_types::c_int,
) -> *mut crate::binary::c_types::c_void {
    create_wifi_queue(queue_len, item_size)
}

/****************************************************************************
 * Name: esp_wifi_delete_queue
 *
 * Description:
 *   Delete Wi-Fi static message queue
 *
 * Input Parameters:
 *   queue - Wi-Fi static message queue data pointer
 *
 * Returned Value:
 *   None
 *
 ****************************************************************************/
pub unsafe extern "C" fn wifi_delete_queue(queue: *mut crate::binary::c_types::c_void) {
    trace!(
        "wifi_delete_queue {:p} - not implemented - doing nothing",
        queue
    );
}

/****************************************************************************
 * Name: wifi_coex_init
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_init() -> crate::binary::c_types::c_int {
    trace!("coex_init - not implemented");
    0
}

/****************************************************************************
 * Name: wifi_coex_deinit
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_deinit() {
    trace!("coex_deinit - not implemented");
}

/****************************************************************************
 * Name: wifi_coex_enable
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_enable() -> crate::binary::c_types::c_int {
    trace!("coex_enable - not implemented");
    0
}

/****************************************************************************
 * Name: wifi_coex_disable
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_disable() {
    todo!("coex_disable")
}

/****************************************************************************
 * Name: esp_coex_status_get
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_status_get() -> u32 {
    trace!("coex_status_get");
    0
}

/****************************************************************************
 * Name: esp_coex_condition_set
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_condition_set(_type_: u32, _dissatisfy: bool) {
    trace!("coex_condition_set - do nothing")
}

/****************************************************************************
 * Name: esp_coex_wifi_request
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_wifi_request(
    _event: u32,
    _latency: u32,
    _duration: u32,
) -> crate::binary::c_types::c_int {
    trace!("coex_wifi_request - not implemented");
    0
}

/****************************************************************************
 * Name: esp_coex_wifi_release
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_wifi_release(_event: u32) -> crate::binary::c_types::c_int {
    trace!("coex_wifi_release - not implemented");
    0
}

/****************************************************************************
 * Name: wifi_coex_wifi_set_channel
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_wifi_channel_set(
    _primary: u8,
    _secondary: u8,
) -> crate::binary::c_types::c_int {
    trace!("coex_wifi_channel_set");
    0
}

/****************************************************************************
 * Name: wifi_coex_get_event_duration
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_event_duration_get(
    _event: u32,
    _duration: *mut u32,
) -> crate::binary::c_types::c_int {
    trace!("coex_event_duration_get");
    // does nothing in original code
    0
}

/****************************************************************************
 * Name: wifi_coex_get_pti
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_pti_get(_event: u32, _pti: *mut u8) -> crate::binary::c_types::c_int {
    trace!("coex_pti_get");
    0
}

/****************************************************************************
 * Name: wifi_coex_clear_schm_status_bit
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_status_bit_clear(_type_: u32, _status: u32) {
    trace!("coex_schm_status_bit_clear")
    // original implementation does nothing here
}

/****************************************************************************
 * Name: wifi_coex_set_schm_status_bit
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_status_bit_set(_type_: u32, _status: u32) {
    trace!("coex_schm_status_bit_set")
    // original implementation does nothing here
}

/****************************************************************************
 * Name: wifi_coex_set_schm_interval
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_interval_set(_interval: u32) -> crate::binary::c_types::c_int {
    todo!("coex_schm_interval_set")
}

/****************************************************************************
 * Name: wifi_coex_get_schm_interval
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_interval_get() -> u32 {
    todo!("coex_schm_interval_get")
}

/****************************************************************************
 * Name: wifi_coex_get_schm_curr_period
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_curr_period_get() -> u8 {
    todo!("coex_schm_curr_period_get")
}

/****************************************************************************
 * Name: wifi_coex_get_schm_curr_phase
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_curr_phase_get() -> *mut crate::binary::c_types::c_void {
    todo!("coex_schm_curr_phase_get")
}

pub unsafe extern "C" fn coex_schm_curr_phase_idx_set(
    _idx: crate::binary::c_types::c_int,
) -> crate::binary::c_types::c_int {
    todo!("coex_schm_curr_phase_idx_set")
}

/****************************************************************************
 * Name: wifi_coex_set_schm_curr_phase_idx
 *
 * Description:
 *   Don't support
 *
 ****************************************************************************/
pub unsafe extern "C" fn coex_schm_curr_phase_idx_get() -> crate::binary::c_types::c_int {
    todo!("coex_schm_curr_phase_idx_get")
}

/****************************************************************************
 * Name: esp_clk_slowclk_cal_get_wrapper
 *
 * Description:
 *   Get the calibration value of RTC slow clock
 *
 * Input Parameters:
 *   None
 *
 * Returned Value:
 *   The calibration value obtained using rtc_clk_cal
 *
 ****************************************************************************/
pub unsafe extern "C" fn slowclk_cal_get() -> u32 {
    trace!("slowclk_cal_get");
    28639
}

// other functions
#[no_mangle]
pub unsafe extern "C" fn puts(s: *const u8) {
    let cstr = StrBuf::from(s);
    trace!("{}", cstr.as_str_ref());
}

#[no_mangle]
pub unsafe extern "C" fn sprintf(dst: *mut u8, format: *const u8, args: ...) -> i32 {
    let str = StrBuf::from(format);
    trace!("sprintf {}", str.as_str_ref());

    let len = crate::compat::common::vsnprintf(dst, 511, format, args);

    let s = StrBuf::from(dst);
    trace!("sprintf {}", s.as_str_ref());

    len
}

#[no_mangle]
pub unsafe extern "C" fn printf(s: *const u8, args: ...) {
    syslog(0, s, args);
}

#[no_mangle]
pub unsafe extern "C" fn phy_printf(s: *const u8, args: ...) {
    syslog(0, s, args);
}

#[no_mangle]
pub unsafe extern "C" fn net80211_printf(s: *const u8, args: ...) {
    syslog(0, s, args);
}

#[no_mangle]
pub unsafe extern "C" fn pp_printf(s: *const u8, args: ...) {
    syslog(0, s, args);
}

// #define ESP_EVENT_DEFINE_BASE(id) esp_event_base_t id = #id
static mut EVT: u8 = 0;
#[no_mangle]
static mut WIFI_EVENT: esp_event_base_t = unsafe { &EVT };

// stuff needed by wpa-supplicant
#[no_mangle]
pub unsafe extern "C" fn __assert_func(
    _file: *const u8,
    _line: u32,
    _func: *const u8,
    _failed_expr: *const u8,
) {
    todo!("__assert_func");
}

#[no_mangle]
pub unsafe extern "C" fn ets_timer_disarm(timer: *mut crate::binary::c_types::c_void) {
    timer_disarm(timer);
}

#[no_mangle]
pub unsafe extern "C" fn ets_timer_done(timer: *mut crate::binary::c_types::c_void) {
    timer_done(timer);
}

#[no_mangle]
pub unsafe extern "C" fn ets_timer_setfn(
    ptimer: *mut crate::binary::c_types::c_void,
    pfunction: *mut crate::binary::c_types::c_void,
    parg: *mut crate::binary::c_types::c_void,
) {
    timer_setfn(ptimer, pfunction, parg);
}

#[no_mangle]
pub unsafe extern "C" fn ets_timer_arm(
    timer: *mut crate::binary::c_types::c_void,
    tmout: u32,
    repeat: bool,
) {
    timer_arm(timer, tmout, repeat);
}

#[no_mangle]
pub unsafe extern "C" fn gettimeofday(_tv: *const (), _tz: *const ()) {
    todo!("gettimeofday");
}

#[no_mangle]
pub unsafe extern "C" fn esp_fill_random(dst: *mut u8, len: u32) {
    trace!("esp_fill_random");
    let dst = core::slice::from_raw_parts_mut(dst, len as usize);

    if let Some(ref mut rng) = RANDOM_GENERATOR {
        rng.read(dst).unwrap();
    }
}

#[no_mangle]
pub unsafe extern "C" fn esp_timer_stop(_handle: *mut ()) {
    todo!("esp_timer_stop");
}

#[no_mangle]
pub unsafe extern "C" fn esp_timer_delete(_handle: *mut ()) {
    todo!("esp_timer_delete");
}

#[no_mangle]
pub unsafe extern "C" fn esp_timer_start_once(_handle: *mut (), _timeout_us: u64) -> i32 {
    todo!("esp_timer_start_once");
}

#[no_mangle]
pub unsafe extern "C" fn esp_timer_create(
    args: *const esp_timer_create_args_t,
    out_handle: *mut esp_timer_handle_t,
) -> i32 {
    compat_esp_timer_create(args, out_handle)
}

#[no_mangle]
pub unsafe extern "C" fn strrchr(_s: *const (), _c: u32) -> *const u8 {
    todo!("strrchr");
}
