fn combine(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) -> i32 {
    let tail = g + h;
    return tail + a;
}

fn main() -> i32 {
    return combine(1, 2, 3, 4, 5, 6, 7, 8);
}
