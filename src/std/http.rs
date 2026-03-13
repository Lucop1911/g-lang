use crate::interpreter::obj::{Object, HashMap};
use crate::errors::RuntimeError;
use std::sync::{Arc, Mutex};
use ahash::HashMapExt;

pub fn http_get(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let args = args;
    Ok(Object::Future(Arc::new(Mutex::new(Some(Box::pin(async_http_get(args)) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<Object, RuntimeError>> + Send + 'static>>)))))
}

async fn async_http_get(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let url = match args.first() {
        Some(Object::String(url)) => url.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 1, max: 1, got: 0 }),
    };

    let client = reqwest::Client::new();
    match client.get(&url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Ok(Object::Hash(create_response_hash(status, body)))
        }
        Err(e) => Err(RuntimeError::InvalidOperation(format!("HTTP GET failed: {}", e)))
    }
}

pub fn http_post(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let args = args;
    Ok(Object::Future(Arc::new(Mutex::new(Some(Box::pin(async_http_post(args)) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<Object, RuntimeError>> + Send + 'static>>)))))
}

async fn async_http_post(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let mut args = args.into_iter();
    
    let url = match args.next() {
        Some(Object::String(url)) => url.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 2, max: 2, got: 0 }),
    };
    
    let body = match args.next() {
        Some(Object::String(body)) => body.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 2, max: 2, got: 1 }),
    };

    let client = reqwest::Client::new();
    match client.post(&url).body(body).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let response_body = response.text().await.unwrap_or_default();
            Ok(Object::Hash(create_response_hash(status, response_body)))
        }
        Err(e) => Err(RuntimeError::InvalidOperation(format!("HTTP POST failed: {}", e)))
    }
}

pub fn http_put(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let args = args;
    Ok(Object::Future(Arc::new(Mutex::new(Some(Box::pin(async_http_put(args)) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<Object, RuntimeError>> + Send + 'static>>)))))
}

async fn async_http_put(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let mut args = args.into_iter();
    
    let url = match args.next() {
        Some(Object::String(url)) => url.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 2, max: 2, got: 0 }),
    };
    
    let body = match args.next() {
        Some(Object::String(body)) => body.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 2, max: 2, got: 1 }),
    };

    let client = reqwest::Client::new();
    match client.put(&url).body(body).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let response_body = response.text().await.unwrap_or_default();
            Ok(Object::Hash(create_response_hash(status, response_body)))
        }
        Err(e) => Err(RuntimeError::InvalidOperation(format!("HTTP PUT failed: {}", e)))
    }
}

pub fn http_delete(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let args = args;
    Ok(Object::Future(Arc::new(Mutex::new(Some(Box::pin(async_http_delete(args)) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<Object, RuntimeError>> + Send + 'static>>)))))
}

async fn async_http_delete(args: Vec<Object>) -> Result<Object, RuntimeError> {
    let url = match args.first() {
        Some(Object::String(url)) => url.clone(),
        Some(o) => return Err(RuntimeError::TypeMismatch { expected: "string".to_string(), got: o.type_name() }),
        None => return Err(RuntimeError::WrongNumberOfArguments { min: 1, max: 1, got: 0 }),
    };

    let client = reqwest::Client::new();
    match client.delete(&url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Ok(Object::Hash(create_response_hash(status, body)))
        }
        Err(e) => Err(RuntimeError::InvalidOperation(format!("HTTP DELETE failed: {}", e)))
    }
}

fn create_response_hash(status: u16, body: String) -> HashMap<Object, Object> {
    let mut hash = HashMap::new();
    hash.insert(Object::String("status".to_string()), Object::Integer(status as i64));
    hash.insert(Object::String("body".to_string()), Object::String(body));
    hash
}
