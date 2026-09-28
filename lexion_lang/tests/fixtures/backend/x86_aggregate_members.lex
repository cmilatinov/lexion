struct Inner {
    first: i32,
    second: i32
}

struct Values {
    reference: &i32,
    inner: Inner
}

fn main() -> i32 {
    let first = 11;
    let second = 23;

    let tuple = (&first, (3, 4));
    let tuple_copy = tuple;
    tuple_copy.0 = &second;
    tuple_copy.1 = (7, 8);
    let tuple_reference = tuple_copy.0;
    let tuple_copy_inner = tuple_copy.1;
    let tuple_inner = tuple_copy.1;
    tuple_inner.0 = 9;

    let values = Values {
        reference: &first,
        inner: Inner { first: 5, second: 6 },
    };
    let values_copy = values;
    values_copy.reference = &second;
    values_copy.inner = Inner { first: 10, second: 12 };
    let values_inner = values_copy.inner;
    values_inner.first = 14;

    let original_tuple_value = *tuple.0;
    let copied_reference_value = *tuple_reference;
    let copied_tuple_value = tuple_copy_inner.0;
    let independent_tuple_value = tuple_inner.0;
    let original_struct_value = values.inner.first;
    let copied_struct_reference_value = *values_copy.reference;
    let copied_struct_value = values_copy.inner.first;
    let independent_struct_value = values_inner.first;

    return original_tuple_value + copied_reference_value + copied_tuple_value
        + independent_tuple_value + original_struct_value + copied_struct_reference_value
        + copied_struct_value + independent_struct_value;
}
