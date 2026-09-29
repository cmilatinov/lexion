struct Point {
    x: i32,
    y: i32
}

fn main() -> i32 {
    let outer = false;
    let inner = false;
    let point = outer
        ? Point { x: 1, y: 2 }
        : inner ? Point { x: 3, y: 4 } : Point { x: 5, y: 6 };
    return point.x;
}
