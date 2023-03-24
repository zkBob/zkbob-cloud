# ZkBob Cloud
A service that allows to manage multiple zkbob accounts in a custodial manner. It can synchronize account's state and perform shielded transfers, but it cannot perform deposits and withdrawals.

## How to start service
1. Create `params` directory and place the transfer parameters (`transfer_params.bin`) there.
2. Create a `.env` file in the `docker` directory. Choose the necessary environment (`runner`, `stage`, `production`) and assign it to the `APP_ENVIRONMENT` variable. Fill in the `ADMIN_TOKEN`. You can always override any of the parameters in `base.yaml` by specifying a new value in the `.env` file. For example, to override the `pool_address` parameter, use the `WEB3__POOL_ADDRESS` variable.
3. To start the service, run the command `docker-compose -f ./docker/docker-compose.yaml up`.

## Configuration
Description of the config parameters can be found in `configuration/base.yaml`.

## API
The available endpoints can be divided into "user" and "admin" categories. User endpoints only require an account id, while to use admin endpoints, you need to provide an `Authorization` header with the value `Bearer ${ADMIN_TOKEN}`.

---
### User API
---
**Retrive account information**

This command initiate sync of the account.

GET: `/account?id=${account_id}`

Response:
```json
{
    "id": "e7da526d-3f46-4f10-adf9-0f4fa9bb15ab",
    "description": "Bob",
    "balance": 10000000000,
    "maxTransferAmount": 9900000000,
    "address": "GwT2R98Q33q5EKKCTBgMqmdz2rRdFPfuWcLJ3Af5TmYu7iDEcS9xn6XQhWKspSA"
}
```
---
**Retrieve account history**

This command initiate sync of the account.

GET: `/history?id=${account_id}`

Response:
```json
[
    {
        "txType": "TransferIn",
        "txHash": "0x2c97b3541f9a0a91517446f18ce49dc3ed73249317754298bb246a4044b72c41",
        "timestamp": 1679649491,
        "amount": 10000000000,
        "to": "9SUHCagSCxhSktVBQcJFBZvZhqDU4wbx3ceyQL4MEa38JSkxEkcyjQMKQsi2nEv"
    },
    {
        "txType": "TransferOut",
        "txHash": "0xedf6004b9498cfafab16890537ef036a82fbfda6c960ecc64ae8c7dd629642da",
        "timestamp": 1679649812,
        "amount": 9900000000,
        "fee": 100000000,
        "to": "KFkNNTLJqBViUUp3BwCYMpc1qF6WZcQiMBGDfW5HbTBj4bUcn5E5rrX5aex2shL",
        "transactionId": "4072da29-d412-4930-a420-df5c18eea74f"
    }
]
```
---
**Generate a shielded address**

GET: `/generateAddress?id=${account_id}`

Response:
```json
{
    "address": "NtYD4uisxHGXWXowLsXjBWMbLf9BFWtu4QwRZTGURFAGp8QhHc6E7jMp4V7UUc8"
}
```
---
**Calculate the transfer fee**

This command initiate sync of the account.

GET: `/calculateFee?accountId=${account_id}&amount=${transfer_amount}`

Response:
```json
{
    "transactionCount": 1,
    "totalFee": 100000000
}
```
---
**Execute a transfer**

This command initiate sync of the account.

POST: `/transfer`

Body:
```json
{
 	"accountId": "${account_id}",
 	"amount": "${transfer_amount}",
 	"to": "${shielded_address}"
}
```

Response:
```json
{
    "transactionId": "ca7ddf90-cba3-4bbc-b28c-c966c461f3e0"
}
```
---
**Get the status of a transaction**

GET: `/transactionStatus?transactionId=${transaction_id}`

Response:
```json
{
    "status": "Done",
    "timestamp": 1679651006,
    "txHash": "0x060be5f1c35879d8aa3140d879ea0d7085a8ef49813d2522162883b020879d91",
    "linkedTxHashes": []
}
```
---
### Admin API
---
**Create a new user account**

The `id` and `sk` parameters are optional.

POST: `/signup`

Body:
```json
{
    "id": null,
    "description": "Bob",
    "sk": null
}
```

Response:
```json
{
    "accountId": "e7da526d-3f46-4f10-adf9-0f4fa9bb15ab"
}
```
---
**Import accounts**

This command can be used to migrate accounts. Additional fields will be ignored.

POST: `/import`

Response: 
```json
[
    {
        "id": "4ab0ea2c-dc70-48f3-8160-980d4f1fed94",
        "description": "AllFi",
        "sk": "3e2a25d79cf0f8d3d4615f7388ffee20d3cce4308c5928973650155481487f02"
    },
    {
        "id": "e7da526d-3f46-4f10-adf9-0f4fa9bb15ab",
        "description": "Bob",
        "sk": "8beb4b3df98a0bb90995507752e626a2cc4055f6ef4d2e0393375f02d5061503"
    }
]
```
---
**List all cloud accounts**

This command does not initiate a sync of all accounts and can be used to export accounts.

GET: `/accounts`

Response:
```json
[
    {
        "id": "4ab0ea2c-dc70-48f3-8160-980d4f1fed94",
        "description": "AllFi",
        "sk": "3e2a25d79cf0f8d3d4615f7388ffee20d3cce4308c5928973650155481487f02"
    },
    {
        "id": "e7da526d-3f46-4f10-adf9-0f4fa9bb15ab",
        "description": "Bob",
        "sk": "8beb4b3df98a0bb90995507752e626a2cc4055f6ef4d2e0393375f02d5061503"
    }
]
```
---
**Delete account**

POST: `/deleteAccount`

Body:
```json
{
    "id": "${account_id}"
}
```

Response status: `OK`

---
**Export account sk**

GET: `/export?id=${account_id}`

Response:
```json
{
    "sk": "3e2a25d79cf0f8d3d4615f7388ffee20d3cce4308c5928973650155481487f02"
}
```
---
**Generate cloud report**

This command syncs all accounts in the background and prepares a report with account balances, keys, and other information.

POST: `/generateReport`

Response:
```json
{
    "id": "32e6c28f-e64f-4099-82f8-7f1d16b0ec5d"
}
```
---
**Get cloud report**

GET: `/report?id=${report_id}`

Response:
```json
{
    "id": "32e6c28f-e64f-4099-82f8-7f1d16b0ec5d",
    "status": "Completed",
    "report": {
        "timestamp": 1679653403,
        "poolIndex": 640,
        "accounts": [
            {
                "id": "4ab0ea2c-dc70-48f3-8160-980d4f1fed94",
                "description": "AllFi",
                "balance": 0,
                "maxTransferAmount": 0,
                "address": "EJJ52BysArxBhcWLRXmVZK8GHHzvDyEGhUKkcGk9roB6yjPdbaXjH58zJ7VWas4",
                "sk": "3e2a25d79cf0f8d3d4615f7388ffee20d3cce4308c5928973650155481487f02"
            },
            {
                "id": "e7da526d-3f46-4f10-adf9-0f4fa9bb15ab",
                "description": "Bob",
                "balance": 0,
                "maxTransferAmount": 0,
                "address": "AXjaGyK2A6NCP9pmp9HWR2kEVWnywWkwvSgzdWLWyu7cvRYNYsG5qk6JMGJs15D",
                "sk": "8beb4b3df98a0bb90995507752e626a2cc4055f6ef4d2e0393375f02d5061503"
            }
        ]
    }
}
```
---
**Clean all reports**

POST: `/cleanReports`

Response status: `OK`

---
### Service API
---
**Health Check**

GET: `/`

Response status: `OK`

---
**Version**

GET: `/version`

Response:
```json
{
    "ref": "main",
    "commitHash": "ab4ce14677621fa3a54f2ceb37da81157d80d7ea"
}
```
---

