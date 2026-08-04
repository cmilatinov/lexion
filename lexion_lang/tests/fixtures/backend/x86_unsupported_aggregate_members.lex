struct Point {
    x: i32
}

struct Values {
    reference: &i32,
    pair: (i32, i32)
}

fn main() -> i32 {
    let value = 1;
    let values = Values {
        reference: &value,
        pair: (2, 3),
    };
    let point = Point { x: 4 };
    let point_reference = &point;
    return (*point_reference).x;
}
