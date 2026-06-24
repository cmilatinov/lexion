struct Point {
    x: i32,
    y: i32
}

fn get_x(point: Point) -> i32 {
    let x = point.x;
    return x;
}

fn tuple_first(pair: (i32, bool)) -> i32 {
    return pair.0;
}
