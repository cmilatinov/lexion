struct Point {
    x: i32,
    y: i32
}

fn main() -> i32 {
    let pair = (3, false);
    let pair_copy = pair;
    pair_copy.0 = 7;
    let point = Point(pair_copy.0, 4);
    let point_copy = point;
    point_copy.y = 9;
    return point_copy.x + point_copy.y;
}
