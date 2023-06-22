//! Common implementation of a stub API server.


impl Stub {
    async fn handle_patch_status(
        &mut self,
        expected_request: impl Expectation,
        network: Network,
    ) -> Result<()> {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        let json: serde_json::Value =
            serde_json::from_str(&request.body.0).expect("status should be JSON");

        let status_json = json.get("status").expect("status object").clone();
        let status: NetworkStatus =
            serde_json::from_value(status_json).expect("JSON should be a valid status");

        let network = network.with_status(status);
        let response = serde_json::to_vec(&network).unwrap();
        send.send_response(
            http::Response::builder()
                .body(Body::from(response))
                .unwrap(),
        );
        Ok(())
    }

    async fn handle_apply(&mut self, expected_request: impl Expectation) -> Result<()> {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        send.send_response(
            http::Response::builder()
                .body(Body::from(request.body.0))
                .unwrap(),
        );
        Ok(())
    }
    async fn handle_request_response<T>(
        &mut self,
        expected_request: impl Expectation,
        response: Option<&T>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        let response = if let Some(response) = response {
            http::Response::builder()
                .body(Body::from(serde_json::to_vec(response).unwrap()))
                .unwrap()
        } else {
            let error = ErrorResponse {
                status: "stub status".to_owned(),
                code: 0,
                message: "stub message".to_owned(),
                reason: "NotFound".to_owned(),
            };
            http::Response::builder()
                .status(404)
                .body(Body::from(serde_json::to_vec(&error).unwrap()))
                .unwrap()
        };
        send.send_response(response);
        Ok(())
    }
}
