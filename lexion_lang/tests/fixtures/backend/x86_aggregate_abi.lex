struct Point {
    x: i32,
    y: i32
}

fn shift(point: Point) -> Point {
    return Point { x: point.x + 1, y: point.y + 2 };
}

fn main() -> i32 {
    let point = Point { x: 3, y: 4 };
    let shifted = shift(point);
    return shifted.x + shifted.y;
}
