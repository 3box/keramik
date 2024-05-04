pub async fn data_feed_benchmark() -> Result<Scenario, GooseError> {
    //Must:
    // 1. Make a transaction that results in a data feed event
    // 2. Increment TPS until data feed performance plateau
    // 3. Keep that metric and share
    let test_start = init_scenario(true).await?;
    let create_new_event = transaction!(create_new_event).set_name(CREATE_EVENT_TX_NAME);
    let stop = transaction!(log_results)
        .set_name("log_results")
        .set_on_stop();
    Ok(scenario!("ReconSync")
        .register_transaction(test_start)
        .register_transaction(create_new_event)
        .register_transaction(stop))
}