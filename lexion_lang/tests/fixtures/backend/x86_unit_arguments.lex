fn unit() {}

fn select(head: (), value: i32, tail: ()) -> i32 {
    return value;
}

fn stack_value(
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32,
    sixth: i32,
    seventh: i32,
    tail: ()
) -> i32 {
    return seventh;
}

fn main() -> i32 {
    let selected = select(unit(), 7, unit());
    return stack_value(1, 2, 3, 4, 5, 6, selected, unit());
}
