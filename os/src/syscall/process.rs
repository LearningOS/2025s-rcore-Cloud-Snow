//! Process management syscalls
use crate::{
    config::PAGE_SIZE,
    mm::{translated_byte_buffer, MapPermission, PageTable, PhysAddr, VirtAddr},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count,
        insert_framed_area, remove_area, suspend_current_and_run_next,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    //获取物理页帧可变引用
    let time_val_size = core::mem::size_of::<TimeVal>();
    let mut buffer = translated_byte_buffer(current_user_token(), _ts as *const u8, time_val_size);

    //计算buffer中切片总长度
    let len = buffer.iter().map(|b| b.len()).sum::<usize>();
    if len != time_val_size {
        trace!("kernel: sys_get_time buffer size error");
        return -1;
    }
    //获取当前时间
    let time = get_time_us();
    let time_val = TimeVal {
        sec: time / 1_000_000,
        usec: time % 1_000_000,
    };
    //将时间值转换为字节数组
    let time_val_bytes =
        unsafe { core::slice::from_raw_parts(&time_val as *const TimeVal as *const u8, len) };
    //将字节数组写入buffer中
    let offset = 0;
    for slice in buffer.iter_mut() {
        let slice_len = slice.len();
        let time_val_slice = &time_val_bytes[offset..offset + slice_len];
        //将时间值写入buffer中
        slice.copy_from_slice(time_val_slice);
    }

    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request {
        0 => {
            // 判断_id对应地址用户是否可见或可读
            let va: VirtAddr = _id.into();
            let page_table = PageTable::from_token(current_user_token());
            let pte = page_table.translate(va.floor());

            if pte.is_none() || !pte.unwrap().is_valid() {
                error!("kernel: sys_trace _id {} is not valid", _id);
                return -1;
            }
            let pte = pte.unwrap();
            if !pte.readable() || !pte.user() {
                error!("kernel: sys_trace _id {} is not readable", _id);
                return -1;
            }

            //获取物理地址
            let pa: usize = PhysAddr::from(pte.ppn()).0 + va.page_offset();
            let ptr = pa as *mut u8;
            //读取数据
            unsafe { *ptr as isize }
        }
        1 => {
            // 判断_id对应地址用户是否可见或可读
            let va: VirtAddr = _id.into();
            let page_table = PageTable::from_token(current_user_token());
            let pte = page_table.translate(va.floor());
            if pte.is_none() || !pte.unwrap().is_valid() {
                error!("kernel: sys_trace _id {} is not valid", _id);
                return -1;
            }
            let pte = pte.unwrap();
            if !pte.writable() || !pte.user() {
                error!("kernel: sys_trace _id {} is not readable", _id);
                return -1;
            }

            //获取物理地址
            let pa: usize = PhysAddr::from(pte.ppn()).0 + va.page_offset();
            let ptr = pa as *mut u8;
            //写入数据
            unsafe {
                *ptr = (_data & 0xFF) as u8;
            }
            0
        }
        2 => get_syscall_count(_id) as isize,
        _ => {
            error!(
                "kernel: sys_trace _trace_request {} is not valid",
                _trace_request
            );
            -1
        }
    }
}

// YOUR JOB: Implement mmap.
/// - 申请长度为 len 字节的物理内存（不要求实际物理内存位置，可以随便找一块），将其映射到 start 开始的虚存，内存页属性为 prot
/// - 参数：
///     - start 需要映射的虚存起始地址，要求按页对齐
///     - len 映射字节长度，可以为 0
///     - prot：第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效且必须为 0
/// - 返回值：执行成功则返回 0，错误返回 -1
/// - 说明：为了简单，目标虚存区间要求按页对齐，len 可直接按页向上取整，不考虑分配失败时的页回收。
/// - 可能的错误：
///     - start 没有按页大小对齐
///     - prot & !0x7 != 0 (prot 其余位必须为0)
///     - prot & 0x7 = 0 (这样的内存无意义)
///     - [start, start + len) 中存在已经被映射的页
///     - 物理内存不足
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    if start % PAGE_SIZE != 0 {
        error!("kernel: sys_mmap start {} is not aligned", start);
        return -1;
    }
    if (prot & !0x7 != 0) || (prot & 0x7) == 0 {
        error!("kernel: sys_mmap prot {} is not valid", prot);
        return -1;
    }
    let permission = prot_to_map_permission(prot);
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    //检查[start, start + len) 中是否存在已经被映射的页
    let page_table = PageTable::from_token(current_user_token());
    let mut va = start_va;
    while va < end_va {
        let pte = page_table.translate(va.floor());
        debug!("kernel: sys_mmap va is {:?}", va);
        if pte.is_some() && pte.unwrap().is_valid() {
            error!(
                "kernel: sys_mmap vpn {:?} is already mapped ppn {:?}. pte flags {:?}",
                va.floor(),
                pte.unwrap().ppn(),
                pte.unwrap().flags()
            );
            return -1;
        }
        va.0 += PAGE_SIZE;
    }
    //申请长度为 len 字节的物理内存，将其映射到 start 开始的虚存
    insert_framed_area(start_va, end_va, permission);
    0
}

fn prot_to_map_permission(prot: usize) -> MapPermission {
    let mut permission = MapPermission::U;
    if prot & 0x1 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        permission |= MapPermission::X;
    }
    permission
}

// YOUR JOB: Implement munmap.
/// 取消到 [start, start + len) 虚存的映射。
/// 特别地，在 rCore 课程实验中，正确执行的 sys_munmap 仅会对应 唯一且完整 的 mmap 区间，不考虑交叉、截断区间的情况
/// 可能的错误：[start, start + len) 中存在未被映射的虚存。
pub fn sys_munmap(start: usize, len: usize) -> isize {
    // 检查[start, start + len) 中是否存在未被映射的虚存
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    let page_table = PageTable::from_token(current_user_token());
    let mut va = start_va;
    while va < end_va {
        let pte = page_table.translate(va.floor());
        debug!("kernel: sys_munmap va is {:?}", va);
        if pte.is_none() || !pte.unwrap().is_valid() {
            error!("kernel: sys_munmap vpn {:?} is not mapped", va.floor());
            return -1;
        }
        va.0 += PAGE_SIZE;
    }
    //取消到 [start, start + len) 虚存的映射
    remove_area(start_va, end_va)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
