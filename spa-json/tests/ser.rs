use spa_json::json;

#[test]
fn test() {
    expect_test::expect![[r#"
        {
          "name": "Alice",
          "age": 30,
          "is_student": false,
          "courses": [
            "Math",
            "Science",
            "Art"
          ],
          "address": {
            "street": "123 Main St",
            "city": "Wonderland"
          }
        }"#]]
    .assert_eq(
        &spa_json::to_string_pretty(&json!({
            "name": "Alice",
            "age": 30,
            "is_student": false,
            "courses": ["Math", "Science", "Art"],
            "address": {
                "street": "123 Main St",
                "city": "Wonderland"
            }
        }))
        .unwrap(),
    )
}
