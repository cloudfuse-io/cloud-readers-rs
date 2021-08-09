# Notebook folder

This folder contains the results of some performance tests conducted on the library when using cloud storage. To make them easily readeable and reproduceable, we used Jupyter notebooks and Docker.

## Deploying the infrastructure

To be able to run the tests with a cloud storage, you first need to deploy the benchmarking executable otherwise you would mainly be measuring your network latency to the cloud.

To run the notebooks, install Jupyter and run `jupyter lab` at the root of the project.

The notebook [`infra.ipynb`](infra.ipynb) helps you deploy an AWS Lambda function into one of your AWS accounts. You just need your credentials to be configured in the default AWS credentials file and change the `AWS_PROFILE` environment variable before starting jupyter to match your setup.

With the same notbook, you can also tear down the infrastructure that was deployed previously. The Terraform state is stored on a local docker volume. If you delete it before running `terraform destroy`, you won't be able to tear down the infrastructure automatically any more.

## Running the benchmarks

You can run the tests by calling the AWS Lambda function deployed previously. 

Note that if you want to run tests on files that are not publicly exposed, you will need to provide the appropriate policy to the Lambda function in the Terraform script.

