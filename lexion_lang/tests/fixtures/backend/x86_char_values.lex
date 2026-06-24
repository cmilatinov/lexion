fn pass(value: char) -> char {
    let local: char = value;
    return local;
}

fn select(first: char, second: char, choose_first: bool) -> char {
    let selected: char = if choose_first { first } else { second };
    return selected;
}

fn main(value: char, fallback: char) -> char {
    let selected: char = select(value, fallback, true);
    return pass(selected);
}
