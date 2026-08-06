struct Padded {
    tag: char,
    address: &i32
}

fn identity(value: Padded) -> Padded {
    return value;
}

fn main(tag: char) -> i32 {
    let number = 7;
    let padded = Padded { tag: tag, address: &number };
    let _returned = identity(padded);
    return number;
}
