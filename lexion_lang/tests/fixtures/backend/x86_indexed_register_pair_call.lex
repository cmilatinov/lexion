struct Quad {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32
}

fn pair_after_stack_f32(
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    g: f32,
    h: f32,
    i: f32,
    value: Quad
) -> i32 {
    return value.first + value.fourth;
}

fn main() -> i32 {
    return pair_after_stack_f32(
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0,
        Quad { first: 3, second: 4, third: 5, fourth: 6 }
    );
}
