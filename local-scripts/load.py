import subprocess

images = [
        'amazon/aws-cli',
        'ceramicnetwork/ceramic-anchor-service:latest',
        'ceramicnetwork/composedb:latest',
        'gresau/localstack-persist:2',
        'public.ecr.aws/r5b3e0r5/3box/cas-contract',
        'public.ecr.aws/r5b3e0r5/3box/ceramic-one',
        'public.ecr.aws/r5b3e0r5/3box/go-cas:latest',
        'trufflesuite/ganache'
        ]


def run_command_for_images(command: list):
    results = []
    for image in images:
        exec = command.copy()
        exec.append(image)
        process = subprocess.Popen(exec, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        results.append(process)
    return results


def check_results(results): 
    for process in results:
        stdout, stderr = process.communicate()
        if process.returncode != 0:
            print("Error: %s" % stderr.decode())


def pull_and_load_images() -> None:
    print("Attempting to load local rust-ceramic, operator and runner images")
    subprocess.run(["kind", "load", "docker-image", "keramik/operator:dev"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    subprocess.run(["kind", "load", "docker-image", "keramik/runner:dev"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    subprocess.run(["kind", "load", "docker-image", "3box/ceramic-one:latest"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    print("Attempting to pull and load other images")
    res = run_command_for_images(["docker", "pull"])
    check_results(res)
    res = run_command_for_images(["kind", "load", "docker-image"])
    check_results(res)


if __name__ == '__main__':
    pull_and_load_images()