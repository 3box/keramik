import os
import sys
import re
from time import sleep


try:
   img_tag = sys.argv[1]

except:
  print("Image tag is required.")
  print("Choose an available tag from https://hub.docker.com/r/ceramicnetwork/js-ceramic/tags")
  exit(0)

label = 'load-with-delay-for-' + img_tag

# replace for valid chars only
label = re.sub(r'\.', '-', label)

os.system('kubectl config set-context --current --namespace=keramik')

# set the image tag
os.system("perl -pi -e 's/js-ceramic:.*$/js-ceramic:{}/g' network-with-cas.yaml".format(img_tag))

# apply the label to the network config 
os.system("perl -pi -e 's/^  name:.*$/  name: {}/g' network-with-cas.yaml".format(label))

# apply the label to the simulation
os.system("perl -pi -e 's/^  namespace:.*$/  namespace: keramik-{}/g' write-only.yaml".format(label))

# create the network
os.system('kubectl apply -f network-with-cas.yaml')

# switch to the network namespace
os.system('kubectl config set-context --current --namespace=keramik-{}'.format(label))

do_edit = """

kubectl patch statefulset cas --type='json' -p='[
  {
    "op": "add",
    "path": "/spec/template/spec/containers/0/env/-",
    "value": {"name": "SQS_QUEUE_URL", "value": ""}
  },
  {
    "op": "add",
    "path": "/spec/template/spec/containers/0/env/-",
    "value": {"name": "MERKLE_CAR_STORAGE_MODE", "value": "disabled"}
  }
]'

"""

os.system(do_edit)

os.system('kubectl label namespace keramik-{} istio-injection=enabled'.format(label))

os.system('kubectl apply -f delay-cas.yaml')

# sleep after pods start to avoid initialization issues
sleep(30)

os.system('kubectl apply -f write-only.yaml')

print("Running simulation for " + label)

print("See https://us3.datadoghq.com/apm/home for results")

print("to clean up in 15 minutes run `kubectl delete -f network-with-cas.html`")




