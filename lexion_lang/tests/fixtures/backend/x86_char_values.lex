fn pass(value: char) -> char {
    let local: char = value;
    return local;
}

fn select(first: char, second: char, choose_first: bool) -> char {
    let selected: char = if choose_first { first } else { second };
    return selected;
}

fn same(left: char, right: char) -> bool {
    return left == right;
}

fn main(value: char, fallback: char) -> char {
    let selected: char = select(value, fallback, true);
    let matched: bool = same(selected, fallback);
    let final: char = if matched { fallback } else { selected };
    return pass(final);
}
