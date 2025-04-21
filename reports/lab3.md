# lab3
## 实验总结
- 修复了sys_get_time sys_mmap sys_munmap
- 实现了sys_pawn，在创建新进程时省去了复制父进程的过程
- 实现了stride调度算法，通过修改idle中run_task的fetch调用实现
- 问题：PageTable中的translate_va实现的有问题，其在使用find_pte后并没有检验pet的有效性就直接获取ppn

## 问答题
1. 不是，因为p2执行一个时间片后,p2.stride+10=260>255，发生了溢出，因此p2.stride=260%255=5，导致p2.stride < p1.stride，因此下一轮还是执行p2
2. 当prioriity>2时，所有进程pass=BigStride/priority < BigStride/2，初始时进程stride都为0，SPRIDE_MAX-SPRID_MIN=0，由于每次都是选择spride最小的进程执行，即SPRIDMIN+=pass，但是pass < BigStride/2，因此|SPRIDE_MAX-SPRID_MIN - pass| < BigStride/2必然成立
3. 考虑溢出，使用 8 bits 存储 stride, BigStride = 255, 则: (125 < 255) == false, (129 < 255) == true.
``` rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let a = self.stride;
        let b = other.stride;
        //计算差值
        let diff = if a > b{
            a - b
        } else {
            b - a
        };

        if diff <= BigStride/2 {
            a < b
        }
        else{   //发生溢出，更小的值其实更大
            b < a
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：
- 无
2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：
- 无

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。