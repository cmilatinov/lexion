fn scale(value: f32, factor: f32) -> f32 {
    return value * factor;
}

fn sum9(
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    g: f32,
    h: f32,
    i: f32
) -> f32 {
    let first = a + b + c + d;
    let second = e + f + g + h;
    return first + second + i;
}

fn f32_tail(
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    g: f32,
    h: f32,
    i: f32,
    tail: i32,
    tail2: i32
) -> i32 {
    let total = a + b + c + d + e + f + g + h + i;
    return total == 45.0 ? tail + tail2 : 0;
}

fn main() -> i32 {
    let scaled = scale(1.5, 2.0);
    let total = sum9(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0);
    let mixed = f32_tail(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 7, 6);
    let correct = scaled == 3.0 && total == 45.0 && mixed == 13;
    return correct ? 0 : 1;
}
