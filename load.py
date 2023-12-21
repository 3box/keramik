
import subprocess

images = [
        'trufflesuite/ganache',
        'gresau/localstack-persist:2',
        'amazon/aws-cli',
        'ceramicnetwork/ceramic-anchor-service:latest',
        'public.ecr.aws/r5b3e0r5/3box/go-cas:latest',
        'public.ecr.aws/r5b3e0r5/3box/cas-contract',
        'ceramicnetwork/composedb:latest',
        'public.ecr.aws/r5b3e0r5/3box/ceramic-one',
        'keramik/operator:dev'
        ]
for image in images:
    print('Pulling image %s' %image)
    subprocess.run(["docker", "pull", image]) 
    print('loading image %s' %image)
    subprocess.run(["kind", "load", "docker-image", image]) 
