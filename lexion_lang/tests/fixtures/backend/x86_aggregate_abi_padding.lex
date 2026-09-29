struct Flagged {
    value: i32,
    flag: bool
}

fn adjust(flagged: Flagged) -> Flagged {
    return Flagged { value: flagged.value + 1, flag: flagged.flag };
}

fn main() -> i32 {
    let flagged = Flagged { value: 3, flag: true };
    let adjusted = adjust(flagged);
    return adjusted.value + adjusted.flag;
}
