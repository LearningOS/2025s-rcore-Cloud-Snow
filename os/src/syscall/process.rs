//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE,
    loader::get_app_data_by_name,
    mm::{
        from_token, translated_byte_buffer, translated_refmut, translated_str, MapPermission,
        VirtAddr,
    },
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, insert_framed_area,
        remove_area, suspend_current_and_run_next,
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
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let pid = current_task().unwrap().pid.0;
    trace!("kernel:pid[{}] sys_get_time", pid);
    //获取物理页帧可变引用
    let time_val_size = core::mem::size_of::<TimeVal>();
    let mut buffer = translated_byte_buffer(current_user_token(), _ts as *const u8, time_val_size);

    //计算buffer中切片总长度
    let len = buffer.iter().map(|b| b.len()).sum::<usize>();
    if len != time_val_size {
        trace!("kernel:pid[{}] sys_get_time buffer size error", pid);
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

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    let pid = current_task().unwrap().pid.0;
    trace!("kernel:pid[{}] sys_mmap", pid);
    if start % PAGE_SIZE != 0 {
        error!(
            "kernel:pid[{}] sys_mmap start {} is not aligned",
            pid, start
        );
        return -1;
    }
    if (prot & !0x7 != 0) || (prot & 0x7) == 0 {
        error!("kernel:pid[{}] sys_mmap prot {} is not valid", pid, prot);
        return -1;
    }
    let permission = prot_to_map_permission(prot);
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    //检查[start, start + len) 中是否存在已经被映射的页
    let page_table = from_token(current_user_token());
    let mut va = start_va;
    while va < end_va {
        let pte = page_table.translate(va.floor());
        debug!("kernel:pid[{}] sys_mmap va is {:?}", pid, va);
        if pte.is_some() && pte.unwrap().is_valid() {
            error!(
                "kernel:pid[{}] sys_mmap vpn {:?} is already mapped ppn {:?}. pte flags {:?}",
                pid,
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

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    let pid = current_task().unwrap().pid.0;
    trace!("kernel:pid[{}] sys_munmap", pid);
    // 检查[start, start + len) 中是否存在未被映射的虚存
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    let page_table = from_token(current_user_token());
    let mut va = start_va;
    while va < end_va {
        let pte = page_table.translate(va.floor());
        debug!("kernel:[{}] sys_munmap va is {:?}", pid, va);
        if pte.is_none() || !pte.unwrap().is_valid() {
            error!(
                "kernel:[{}] sys_munmap vpn {:?} is not mapped",
                pid,
                va.floor()
            );
            return -1;
        }
        va.0 += PAGE_SIZE;
    }
    //取消到 [start, start + len) 虚存的映射
    remove_area(start_va, end_va)
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    let pid = current_task().unwrap().pid.0;
    trace!("kernel:pid[{}] sys_spawn", pid);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let new_task = current_task().unwrap().spawn(data);
        let new_pid = new_task.getpid();
        let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
        trap_cx.x[10] = 0;
        add_task(new_task);
        new_pid as isize
    } else {
        error!("kernel:pid[{}] sys_spawn path {} is not valid", pid, path);
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}
