struct Large {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32
}

struct Quad {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32
}

fn large_after_gprs(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, value: Large) -> i32 {
    return a + b + c + d + e + f + value.first + value.fifth;
}

fn pair_after_gprs(a: i32, b: i32, c: i32, d: i32, e: i32, value: Quad) -> i32 {
    return a + b + c + d + e + value.first + value.fourth;
}

fn f32_after_stack_pair(a: i32, b: i32, c: i32, d: i32, e: i32, value: Quad, tail: f32) -> f32 {
    return tail;
}

fn main() -> i32 {
    let large = Large { first: 7, second: 8, third: 9, fourth: 10, fifth: 11 };
    let quad = Quad { first: 12, second: 13, third: 14, fourth: 15 };
    let tail = f32_after_stack_pair(1, 2, 3, 4, 5, quad, 12.0);
    let total = large_after_gprs(1, 2, 3, 4, 5, 6, large) + pair_after_gprs(1, 2, 3, 4, 5, quad);
    if tail == 12.0 {
        return total;
    } else {
        return 0;
    }
}
