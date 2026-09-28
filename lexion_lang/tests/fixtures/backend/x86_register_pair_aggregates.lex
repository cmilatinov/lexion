struct Quad {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32
}

fn shift_quad(value: Quad) -> Quad {
    return Quad {
        first: value.first + 1,
        second: value.second + 2,
        third: value.third + 3,
        fourth: value.fourth + 4
    };
}

fn shift_tuple(value: (i32, i32, i32)) -> (i32, i32, i32) {
    return (value.0 + 1, value.1 + 2, value.2 + 3);
}

fn main() -> i32 {
    let quad = shift_quad(Quad { first: 1, second: 2, third: 3, fourth: 4 });
    let tuple = shift_tuple((5, 6, 7));
    return quad.first + quad.second + quad.third + quad.fourth + tuple.0 + tuple.1 + tuple.2;
}
