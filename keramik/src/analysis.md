# Analysis
Analysis of Keramik results depends on the purpose of the simulation. You may want to just see average latencies, or dive
deeper into reported metrics. For profiling, you will want to use [datadog](./datadog.md).

## Quick Log Analysis
The simulation manager provides a very quick way to analyze the logs of a simulation run. You will need to know the name
of the manager pod though. You will first need to see if the `simulate-manager` pod has completed, by running

    kubectl get pods

If the pod has completed and is no longer in that list, you can see recently terminated pods using:

    kubectl get event -o custom-columns=NAME:.metadata.name | cut -d "." -f1

Once you have the name of the manager, you can retrieve its logs

    kubectl logs simulate-manager-<id>

If the `simulate-manager` pod is not in your pod list, you may need to get logs with the `--previous` flag:

    kubectl logs --previous simulate-manager-<id>

## Analysis with DuckDB or Jupyter

First you will need to install a few things:

    pip install duckdb duckdb-engine pandas jupyter jupysql matplotlib

To analyze the results of a simulation first copy the metrics-TIMESTAMP.parquet file from the otel-0 pod.
First restart opentelemetry-0 pod so it writes out the parquet file footer.

    kubectl delete pod opentelemetry-0
    kubectl wait --for=condition=Ready pod/opentelemetry-0 # make sure pod has restarted
    kubectl exec opentelemetry-0 -- ls -la /data # List files in the directly find the TIMESTAMP you need
    kubectl cp opentelemetry-0:data/metrics-TIMESTAMP.parquet ./analyze/metrics.parquet
    cd analyze

Use duckdb to examine the data:

    duckdb
    > SELECT * FROM 'metrics.parquet' LIMIT 10;

Alternatively start a jupyter notebook using `analyze/sim.ipynb`:

    jupyter notebook

## Comparing Simulation Runs

How do we conclude a simulation is better or worse that another run?

Each simulation will likely be targeting a specific result however there are common results we should expect to see.

Changes should not make correctness worse. Correctness is defined using two metrics:

- Percentage of events successfully persisted on the node that accepted the initial write.
- Percentage of events successfully replicated on nodes that observed the writes via the Ceramic protocol.


Changes should not make performance worse. Performance is defined using these metrics:

- Writes/sec across all nodes in the cluster and by node
- p50,p90,p95,p99 and p99.9 of the duration of writes across all nodes in the cluster and by node
- Success/failure ratio of writes requests across all nodes in the cluster and by node
- p50,p90,p95,p99 and p99.9 of duration of time to become replicated. The time from when one node accepts the write to when another node has the same write available for read.


For any simulation of the Ceramic protocol these metrics should apply. Any report about the results of a simulation should include these metrics and we compare them against the established a baseline.

## Performance Analysis
In addition to the above, we can also use [datadog](./datadog.md) to dive further into performance.