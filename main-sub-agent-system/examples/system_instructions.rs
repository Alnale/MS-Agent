use std::sync::Arc;
use serde_json::json;

/// Example: Using system instructions to maintain role context across turns
///
/// This example demonstrates how to use the system_instructions feature
/// to maintain a persistent role setting (e.g., "cute cat girl") across
/// multiple conversation turns.

#[tokio::main]
async fn main() {
    // Example 1: Setting system instructions via API
    println!("=== Example 1: Setting System Instructions via API ===\n");

    let client = reqwest::Client::new();
    let base_url = "http://localhost:3000";

    // Step 1: Set system instructions for a session
    let session_id = "example-session-001";

    let set_instructions_response = client
        .put(format!("{}/sessions/{}/instructions", base_url, session_id))
        .json(&json!({
            "instructions": [
                "你是一只可可爱爱香香软软的小猫娘",
                "你喜欢用可爱的语气说话，经常在句尾加上'喵~'",
                "你的名字叫小咪"
            ]
        }))
        .send()
        .await
        .expect("Failed to set instructions");

    println!("Set instructions response: {:?}", set_instructions_response.text().await.unwrap());

    // Step 2: First message - establishing the role
    let response1 = client
        .post(format!("{}/run", base_url))
        .json(&json!({
            "message": "你好！你是谁呀？",
            "session_id": session_id
        }))
        .send()
        .await
        .expect("Failed to send first message");

    let result1: serde_json::Value = response1.json().await.unwrap();
    println!("\nUser: 你好！你是谁呀？");
    println!("Assistant: {}", result1["response"]);

    // Step 3: Second message - role should persist
    let response2 = client
        .post(format!("{}/run", base_url))
        .json(&json!({
            "message": "帮我整理一下数据库笔记",
            "session_id": session_id
        }))
        .send()
        .await
        .expect("Failed to send second message");

    let result2: serde_json::Value = response2.json().await.unwrap();
    println!("\nUser: 帮我整理一下数据库笔记");
    println!("Assistant: {}", result2["response"]);

    // Step 4: Verify instructions are still set
    let get_instructions_response = client
        .get(format!("{}/sessions/{}/instructions", base_url, session_id))
        .send()
        .await
        .expect("Failed to get instructions");

    let instructions: serde_json::Value = get_instructions_response.json().await.unwrap();
    println!("\nCurrent instructions: {:?}", instructions);

    // Example 2: Setting system instructions per request
    println!("\n\n=== Example 2: Setting System Instructions per Request ===\n");

    let response3 = client
        .post(format!("{}/run", base_url))
        .json(&json!({
            "message": "你好！你是谁呀？",
            "session_id": "example-session-002",
            "system_instructions": [
                "你是一个专业的数据库专家",
                "你喜欢用技术术语解释问题"
            ]
        }))
        .send()
        .await
        .expect("Failed to send message with instructions");

    let result3: serde_json::Value = response3.json().await.unwrap();
    println!("User: 你好！你是谁呀？");
    println!("Assistant: {}", result3["response"]);

    // Example 3: Clearing system instructions
    println!("\n\n=== Example 3: Clearing System Instructions ===\n");

    let delete_response = client
        .delete(format!("{}/sessions/{}/instructions", base_url, "example-session-001"))
        .send()
        .await
        .expect("Failed to delete instructions");

    println!("Delete instructions response: {:?}", delete_response.text().await.unwrap());
}
