struct Large {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32
}

fn large_before_f32(
    a: i32,
    b: i32,
    c: i32,
    d: i32,
    e: i32,
    f: i32,
    value: Large,
    tail: f32
) -> i32 {
    return a + b + c + d + e + f + value.first + value.fifth;
}

fn main() -> i32 {
    return large_before_f32(
        1, 2, 3, 4, 5, 6,
        Large { first: 7, second: 8, third: 9, fourth: 10, fifth: 11 },
        12.0
    );
}
