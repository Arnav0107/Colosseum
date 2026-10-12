/**
 * Program IDL in camelCase format in order to be used in JS/TS.
 *
 * Note that this is only a type helper and is not the actual IDL. The original
 * IDL can be found at `target/idl/ch_core.json`.
 */
export type ChCore = {
  "address": "2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg",
  "metadata": {
    "name": "chCore",
    "version": "0.1.0",
    "spec": "0.1.0"
  },
  "instructions": [
    {
      "name": "acceptAdmin",
      "discriminator": [
        112,
        42,
        45,
        90,
        116,
        181,
        13,
        170
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "proposedAdmin",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "activateVenue",
      "discriminator": [
        151,
        81,
        158,
        156,
        32,
        171,
        169,
        52
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "account",
                "path": "venueRegistration.venueId",
                "account": "venueRegistration"
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "applyPendingChange",
      "discriminator": [
        93,
        46,
        110,
        164,
        104,
        13,
        223,
        12
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "bumpRiskEpoch",
      "discriminator": [
        80,
        14,
        37,
        100,
        134,
        23,
        6,
        164
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "cancelPendingChange",
      "discriminator": [
        213,
        20,
        165,
        118,
        85,
        59,
        178,
        41
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "closePositionSnapshot",
      "discriminator": [
        90,
        173,
        170,
        217,
        162,
        159,
        171,
        178
      ],
      "accounts": [
        {
          "name": "venueRegistration",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "account",
                "path": "venueRegistration.venueId",
                "account": "venueRegistration"
              }
            ]
          }
        },
        {
          "name": "userConsent",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "posSnapshot.user",
                "account": "posSnapshot"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "posSnapshot.user",
                "account": "posSnapshot"
              }
            ]
          }
        },
        {
          "name": "posSnapshot",
          "writable": true
        },
        {
          "name": "rentReceiver",
          "writable": true
        },
        {
          "name": "closer",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "closeUserAccounts",
      "docs": [
        "Close consent + portfolio (all snapshots must be closed first)."
      ],
      "discriminator": [
        216,
        186,
        79,
        49,
        148,
        195,
        201,
        168
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "userConsent",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "user",
          "writable": true,
          "signer": true,
          "relations": [
            "userConsent",
            "portfolio"
          ]
        }
      ],
      "args": []
    },
    {
      "name": "deactivateVenue",
      "discriminator": [
        196,
        144,
        81,
        124,
        183,
        250,
        190,
        218
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "account",
                "path": "venueRegistration.venueId",
                "account": "venueRegistration"
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "initialize",
      "docs": [
        "A7: the three operational roles are set here; production builds",
        "require all four keys (with the admin) to be distinct."
      ],
      "discriminator": [
        175,
        175,
        109,
        31,
        13,
        152,
        155,
        237
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "admin",
          "writable": true,
          "signer": true
        },
        {
          "name": "systemProgram",
          "address": "11111111111111111111111111111111"
        }
      ],
      "args": [
        {
          "name": "params",
          "type": {
            "defined": {
              "name": "configParams"
            }
          }
        },
        {
          "name": "riskAuthority",
          "type": "pubkey"
        },
        {
          "name": "venueManager",
          "type": "pubkey"
        },
        {
          "name": "guardian",
          "type": "pubkey"
        }
      ]
    },
    {
      "name": "invalidatePortfolio",
      "discriminator": [
        51,
        125,
        235,
        70,
        178,
        47,
        218,
        141
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "docs": [
            "Source of the governed oracle feed per asset."
          ],
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "userConsent",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "portfolio.user",
                "account": "portfolio"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "portfolio.user",
                "account": "portfolio"
              }
            ]
          }
        }
      ],
      "args": []
    },
    {
      "name": "pause",
      "discriminator": [
        211,
        22,
        221,
        251,
        74,
        121,
        193,
        47
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "proposeAdmin",
      "discriminator": [
        121,
        214,
        199,
        212,
        87,
        39,
        117,
        234
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "admin",
          "signer": true,
          "relations": [
            "config"
          ]
        }
      ],
      "args": [
        {
          "name": "newAdmin",
          "type": "pubkey"
        }
      ]
    },
    {
      "name": "refreshPortfolio",
      "discriminator": [
        156,
        111,
        124,
        206,
        19,
        223,
        90,
        55
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "userConsent",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "portfolio.user",
                "account": "portfolio"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "portfolio.user",
                "account": "portfolio"
              }
            ]
          }
        }
      ],
      "args": []
    },
    {
      "name": "registerVenue",
      "discriminator": [
        116,
        155,
        199,
        243,
        228,
        180,
        176,
        173
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "arg",
                "path": "venueId"
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        },
        {
          "name": "payer",
          "writable": true,
          "signer": true
        },
        {
          "name": "systemProgram",
          "address": "11111111111111111111111111111111"
        }
      ],
      "args": [
        {
          "name": "venueId",
          "type": {
            "array": [
              "u8",
              32
            ]
          }
        },
        {
          "name": "venueProgramId",
          "type": "pubkey"
        },
        {
          "name": "venueAuthority",
          "type": "pubkey"
        },
        {
          "name": "venueIndex",
          "type": "u8"
        }
      ]
    },
    {
      "name": "requestVenueActivation",
      "discriminator": [
        14,
        152,
        236,
        69,
        204,
        136,
        114,
        223
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "account",
                "path": "venueRegistration.venueId",
                "account": "venueRegistration"
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": []
    },
    {
      "name": "setAssetRisk",
      "discriminator": [
        183,
        93,
        211,
        163,
        242,
        97,
        209,
        125
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "assetId",
          "type": "u8"
        },
        {
          "name": "params",
          "type": {
            "defined": {
              "name": "assetRiskArgs"
            }
          }
        }
      ]
    },
    {
      "name": "setMockPrice",
      "discriminator": [
        161,
        22,
        71,
        90,
        159,
        254,
        26,
        48
      ],
      "accounts": [
        {
          "name": "config",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "mockPrice",
          "writable": true
        },
        {
          "name": "admin",
          "writable": true,
          "signer": true,
          "relations": [
            "config"
          ]
        },
        {
          "name": "systemProgram",
          "address": "11111111111111111111111111111111"
        }
      ],
      "args": [
        {
          "name": "assetId",
          "type": "u8"
        },
        {
          "name": "priceMicro",
          "type": "u64"
        },
        {
          "name": "confMicro",
          "type": "u64"
        },
        {
          "name": "publishTs",
          "type": "i64"
        }
      ]
    },
    {
      "name": "setOracleFeed",
      "docs": [
        "Pyth feed id for an asset. Setting or switching a feed waits for the",
        "timelock; clearing it (all zeros) applies at once."
      ],
      "discriminator": [
        115,
        216,
        56,
        6,
        24,
        198,
        170,
        38
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "assetId",
          "type": "u8"
        },
        {
          "name": "feedId",
          "type": {
            "array": [
              "u8",
              32
            ]
          }
        }
      ]
    },
    {
      "name": "setRoles",
      "discriminator": [
        119,
        86,
        129,
        161,
        55,
        23,
        250,
        12
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "admin",
          "signer": true,
          "relations": [
            "config"
          ]
        }
      ],
      "args": [
        {
          "name": "riskAuthority",
          "type": "pubkey"
        },
        {
          "name": "venueManager",
          "type": "pubkey"
        },
        {
          "name": "guardian",
          "type": "pubkey"
        }
      ]
    },
    {
      "name": "setScenario",
      "discriminator": [
        109,
        102,
        56,
        143,
        25,
        94,
        176,
        190
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "index",
          "type": "u8"
        },
        {
          "name": "kind",
          "type": "u8"
        },
        {
          "name": "moves",
          "type": {
            "vec": {
              "defined": {
                "name": "assetMove"
              }
            }
          }
        }
      ]
    },
    {
      "name": "setStressParams",
      "discriminator": [
        116,
        253,
        20,
        109,
        105,
        128,
        195,
        171
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "corrStressPpm",
          "type": "u32"
        },
        {
          "name": "basisDecorrPpm",
          "type": "u32"
        }
      ]
    },
    {
      "name": "setVenueAcceptance",
      "docs": [
        "A1: venues whose reports this venue accepts credit from (own bit",
        "implied). Signer: the venue's registered authority."
      ],
      "discriminator": [
        189,
        181,
        155,
        149,
        251,
        202,
        67,
        39
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "account",
                "path": "venueRegistration.venueId",
                "account": "venueRegistration"
              }
            ]
          }
        },
        {
          "name": "venueAuthority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "acceptsMask",
          "type": "u64"
        }
      ]
    },
    {
      "name": "submitPositionSnapshot",
      "docs": [
        "A5: `mark_price_micro` (0 = not reported) is checked against the",
        "asset's oracle, passed as the first remaining account."
      ],
      "discriminator": [
        232,
        58,
        29,
        242,
        23,
        228,
        111,
        118
      ],
      "accounts": [
        {
          "name": "config",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "venueRegistration",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  118,
                  101,
                  110,
                  117,
                  101
                ]
              },
              {
                "kind": "arg",
                "path": "venueId"
              }
            ]
          }
        },
        {
          "name": "userConsent",
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "posSnapshot",
          "writable": true
        },
        {
          "name": "user",
          "docs": [
            "portfolio and snapshot PDA seeds."
          ]
        },
        {
          "name": "venueAuthority",
          "docs": [
            "With `venue-cpi` (production) this is the venue program's",
            "[b\"clearinghouse\"] PDA (enforced at registration), so only that program",
            "can sign, through CPI. With `dev-venue-signer` it may be a keypair."
          ],
          "signer": true
        },
        {
          "name": "payer",
          "writable": true,
          "signer": true
        },
        {
          "name": "systemProgram",
          "address": "11111111111111111111111111111111"
        },
        {
          "name": "riskModel",
          "docs": [
            "A5d: source of the governed oracle feed for the mark-price check."
          ],
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        }
      ],
      "args": [
        {
          "name": "venueId",
          "type": {
            "array": [
              "u8",
              32
            ]
          }
        },
        {
          "name": "assetId",
          "type": "u8"
        },
        {
          "name": "notional",
          "type": "u64"
        },
        {
          "name": "isLong",
          "type": "bool"
        },
        {
          "name": "initialMargin",
          "type": "u64"
        },
        {
          "name": "maintenanceMargin",
          "type": "u64"
        },
        {
          "name": "markPriceMicro",
          "type": "u64"
        }
      ]
    },
    {
      "name": "unpause",
      "discriminator": [
        169,
        144,
        4,
        38,
        10,
        141,
        188,
        255
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "admin",
          "signer": true,
          "relations": [
            "config"
          ]
        }
      ],
      "args": []
    },
    {
      "name": "updateCorrelations",
      "discriminator": [
        115,
        142,
        33,
        40,
        56,
        60,
        20,
        255
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "entries",
          "type": {
            "vec": {
              "defined": {
                "name": "corrEntry"
              }
            }
          }
        }
      ]
    },
    {
      "name": "updateParams",
      "discriminator": [
        108,
        178,
        190,
        95,
        94,
        203,
        116,
        20
      ],
      "accounts": [
        {
          "name": "config",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  102,
                  105,
                  103
                ]
              }
            ]
          }
        },
        {
          "name": "riskModel",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  114,
                  105,
                  115,
                  107,
                  95,
                  109,
                  111,
                  100,
                  101,
                  108
                ]
              }
            ]
          }
        },
        {
          "name": "pendingChange",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  101,
                  110,
                  100,
                  105,
                  110,
                  103,
                  95,
                  99,
                  104,
                  97,
                  110,
                  103,
                  101
                ]
              }
            ]
          }
        },
        {
          "name": "authority",
          "signer": true
        }
      ],
      "args": [
        {
          "name": "params",
          "type": {
            "defined": {
              "name": "configParams"
            }
          }
        }
      ]
    },
    {
      "name": "updateUserConsent",
      "discriminator": [
        22,
        221,
        94,
        211,
        131,
        177,
        82,
        83
      ],
      "accounts": [
        {
          "name": "userConsent",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  99,
                  111,
                  110,
                  115,
                  101,
                  110,
                  116
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "portfolio",
          "writable": true,
          "pda": {
            "seeds": [
              {
                "kind": "const",
                "value": [
                  112,
                  111,
                  114,
                  116,
                  102,
                  111,
                  108,
                  105,
                  111
                ]
              },
              {
                "kind": "account",
                "path": "user"
              }
            ]
          }
        },
        {
          "name": "user",
          "writable": true,
          "signer": true
        },
        {
          "name": "systemProgram",
          "address": "11111111111111111111111111111111"
        }
      ],
      "args": [
        {
          "name": "isActive",
          "type": "bool"
        },
        {
          "name": "authorizedVenuesBitmap",
          "type": "u64"
        }
      ]
    }
  ],
  "accounts": [
    {
      "name": "globalConfig",
      "discriminator": [
        149,
        8,
        156,
        202,
        160,
        252,
        176,
        217
      ]
    },
    {
      "name": "mockPrice",
      "discriminator": [
        2,
        6,
        235,
        132,
        88,
        250,
        211,
        139
      ]
    },
    {
      "name": "pendingChange",
      "discriminator": [
        232,
        183,
        176,
        196,
        15,
        195,
        115,
        147
      ]
    },
    {
      "name": "portfolio",
      "discriminator": [
        94,
        158,
        71,
        245,
        122,
        102,
        110,
        225
      ]
    },
    {
      "name": "posSnapshot",
      "discriminator": [
        81,
        202,
        108,
        9,
        160,
        154,
        250,
        59
      ]
    },
    {
      "name": "riskModel",
      "discriminator": [
        72,
        76,
        108,
        40,
        223,
        44,
        80,
        244
      ]
    },
    {
      "name": "userConsent",
      "discriminator": [
        133,
        149,
        46,
        15,
        180,
        18,
        40,
        103
      ]
    },
    {
      "name": "venueRegistration",
      "discriminator": [
        203,
        111,
        167,
        57,
        223,
        196,
        74,
        143
      ]
    }
  ],
  "events": [
    {
      "name": "adminProposed",
      "discriminator": [
        129,
        249,
        226,
        227,
        199,
        82,
        110,
        243
      ]
    },
    {
      "name": "adminUpdated",
      "discriminator": [
        69,
        82,
        49,
        171,
        43,
        3,
        80,
        161
      ]
    },
    {
      "name": "changeApplied",
      "discriminator": [
        226,
        1,
        224,
        100,
        192,
        110,
        58,
        24
      ]
    },
    {
      "name": "changeCancelled",
      "discriminator": [
        139,
        179,
        97,
        81,
        214,
        21,
        119,
        93
      ]
    },
    {
      "name": "changeQueued",
      "discriminator": [
        232,
        143,
        80,
        230,
        125,
        117,
        169,
        213
      ]
    },
    {
      "name": "consentUpdated",
      "discriminator": [
        61,
        124,
        224,
        3,
        170,
        177,
        128,
        187
      ]
    },
    {
      "name": "emergencyEpochBump",
      "discriminator": [
        253,
        153,
        160,
        34,
        119,
        163,
        218,
        203
      ]
    },
    {
      "name": "paramsUpdated",
      "discriminator": [
        2,
        163,
        138,
        99,
        135,
        11,
        136,
        169
      ]
    },
    {
      "name": "pausedUpdated",
      "discriminator": [
        209,
        89,
        200,
        35,
        126,
        183,
        132,
        23
      ]
    },
    {
      "name": "portfolioInvalidated",
      "discriminator": [
        26,
        94,
        167,
        49,
        150,
        181,
        13,
        87
      ]
    },
    {
      "name": "portfolioRefreshed",
      "discriminator": [
        240,
        119,
        252,
        144,
        155,
        96,
        106,
        219
      ]
    },
    {
      "name": "positionSnapshotClosed",
      "discriminator": [
        170,
        236,
        245,
        120,
        111,
        211,
        41,
        246
      ]
    },
    {
      "name": "positionSnapshotSubmitted",
      "discriminator": [
        32,
        128,
        50,
        176,
        5,
        44,
        74,
        148
      ]
    },
    {
      "name": "riskModelUpdated",
      "discriminator": [
        93,
        203,
        92,
        153,
        72,
        247,
        137,
        189
      ]
    },
    {
      "name": "rolesUpdated",
      "discriminator": [
        81,
        37,
        176,
        32,
        30,
        204,
        251,
        246
      ]
    },
    {
      "name": "userAccountsClosed",
      "discriminator": [
        133,
        234,
        48,
        42,
        199,
        75,
        191,
        78
      ]
    },
    {
      "name": "venueAcceptanceUpdated",
      "discriminator": [
        166,
        44,
        88,
        95,
        218,
        158,
        206,
        160
      ]
    },
    {
      "name": "venueActiveUpdated",
      "discriminator": [
        209,
        175,
        239,
        241,
        110,
        235,
        126,
        249
      ]
    },
    {
      "name": "venueRegistered",
      "discriminator": [
        86,
        82,
        191,
        91,
        171,
        241,
        178,
        9
      ]
    }
  ],
  "errors": [
    {
      "code": 6000,
      "name": "unauthorized",
      "msg": "Unauthorized access"
    },
    {
      "code": 6001,
      "name": "venueInactive",
      "msg": "Venue is not active"
    },
    {
      "code": 6002,
      "name": "userConsentMissing",
      "msg": "User consent missing or inactive"
    },
    {
      "code": 6003,
      "name": "venueNotAuthorized",
      "msg": "Venue not authorized in user consent"
    },
    {
      "code": 6004,
      "name": "invalidVenueIndex",
      "msg": "Invalid venue index"
    },
    {
      "code": 6005,
      "name": "invalidAssetId",
      "msg": "Invalid asset ID"
    },
    {
      "code": 6006,
      "name": "invalidCorrelationValue",
      "msg": "Invalid correlation value"
    },
    {
      "code": 6007,
      "name": "riskModelStale",
      "msg": "Risk model is stale"
    },
    {
      "code": 6008,
      "name": "priceStale",
      "msg": "Price oracle is stale"
    },
    {
      "code": 6009,
      "name": "priceUncertain",
      "msg": "Price oracle confidence interval too wide"
    },
    {
      "code": 6010,
      "name": "invalidPrice",
      "msg": "Price cannot be zero or negative"
    },
    {
      "code": 6011,
      "name": "oracleNotConfigured",
      "msg": "Oracle backend not configured"
    },
    {
      "code": 6012,
      "name": "invalidOracleAccount",
      "msg": "Invalid oracle account"
    },
    {
      "code": 6013,
      "name": "mathOverflow",
      "msg": "Integer overflow in calculation"
    },
    {
      "code": 6014,
      "name": "negativeVariance",
      "msg": "Negative variance in portfolio netting"
    },
    {
      "code": 6015,
      "name": "snapshotStale",
      "msg": "Snapshot is stale"
    },
    {
      "code": 6016,
      "name": "invalidSnapshotAccount",
      "msg": "Invalid snapshot account"
    },
    {
      "code": 6017,
      "name": "zeroNotional",
      "msg": "Snapshot notional cannot be zero"
    },
    {
      "code": 6018,
      "name": "invalidMargin",
      "msg": "Margins must satisfy 0 < maintenance <= initial <= notional"
    },
    {
      "code": 6019,
      "name": "invalidConfigParams",
      "msg": "Invalid configuration parameters"
    },
    {
      "code": 6020,
      "name": "venueIndexAlreadyUsed",
      "msg": "Venue index is already used"
    },
    {
      "code": 6021,
      "name": "programPaused",
      "msg": "Clearinghouse is paused"
    },
    {
      "code": 6022,
      "name": "invalidAdmin",
      "msg": "Invalid proposed admin address"
    },
    {
      "code": 6023,
      "name": "portfolioFull",
      "msg": "Portfolio already has the maximum number of legs"
    },
    {
      "code": 6024,
      "name": "duplicateLeg",
      "msg": "Duplicate leg (venue, asset) in portfolio"
    },
    {
      "code": 6025,
      "name": "incompletePortfolio",
      "msg": "Refresh must include exactly the portfolio's registered snapshots, then one oracle per leg"
    },
    {
      "code": 6026,
      "name": "tooManyLegs",
      "msg": "Too many legs"
    },
    {
      "code": 6027,
      "name": "invalidationNotJustified",
      "msg": "No invalidation condition holds"
    },
    {
      "code": 6028,
      "name": "counterUnderflow",
      "msg": "Outstanding-credit counter would underflow"
    },
    {
      "code": 6029,
      "name": "snapshotCloseNotAllowed",
      "msg": "Caller may not close this snapshot"
    },
    {
      "code": 6030,
      "name": "unsupportedAsset",
      "msg": "Asset has no risk parameters (shock_bps = 0)"
    },
    {
      "code": 6031,
      "name": "invalidRiskParams",
      "msg": "Risk parameter outside its allowed bounds"
    },
    {
      "code": 6032,
      "name": "invalidScenario",
      "msg": "Scenario index out of range"
    },
    {
      "code": 6033,
      "name": "changeTooLarge",
      "msg": "Change exceeds the per-update step limit"
    },
    {
      "code": 6034,
      "name": "pendingChangeExists",
      "msg": "Another change is already pending"
    },
    {
      "code": 6035,
      "name": "noPendingChange",
      "msg": "No change is pending"
    },
    {
      "code": 6036,
      "name": "timelockNotElapsed",
      "msg": "Timelock has not elapsed"
    },
    {
      "code": 6037,
      "name": "venueAuthorityNotProgramPda",
      "msg": "Venue authority must be the venue program's [\\clearinghouse\\] PDA"
    },
    {
      "code": 6038,
      "name": "oracleNotFullyVerified",
      "msg": "Pyth price update is not fully verified"
    },
    {
      "code": 6039,
      "name": "snapshotsStillOpen",
      "msg": "Close every position snapshot before closing the user's accounts"
    },
    {
      "code": 6040,
      "name": "rolesNotDistinct",
      "msg": "Production builds require admin, risk_authority, venue_manager and guardian to be four distinct keys"
    },
    {
      "code": 6041,
      "name": "legNotionalCapExceeded",
      "msg": "Reported notional exceeds max_leg_notional"
    },
    {
      "code": 6042,
      "name": "venueNotionalCapExceeded",
      "msg": "This user's notional on this venue would exceed max_venue_notional_per_user"
    },
    {
      "code": 6043,
      "name": "notionalGrowthTooFast",
      "msg": "Reported notional grew faster than max_notional_growth_bps allows in this window"
    },
    {
      "code": 6044,
      "name": "markPriceDeviation",
      "msg": "Venue-reported mark price differs from the oracle by more than max_move_bps"
    },
    {
      "code": 6045,
      "name": "markPriceOracleMissing",
      "msg": "A mark price was reported without the asset's oracle account"
    }
  ],
  "types": [
    {
      "name": "adminProposed",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "admin",
            "type": "pubkey"
          },
          {
            "name": "proposedAdmin",
            "type": "pubkey"
          }
        ]
      }
    },
    {
      "name": "adminUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "oldAdmin",
            "type": "pubkey"
          },
          {
            "name": "newAdmin",
            "type": "pubkey"
          }
        ]
      }
    },
    {
      "name": "assetMove",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "asset",
            "type": "u8"
          },
          {
            "name": "moveBps",
            "docs": [
              "Signed move in bps, in [-10_000, 10_000]."
            ],
            "type": "i16"
          }
        ]
      }
    },
    {
      "name": "assetRiskArgs",
      "docs": [
        "Instruction argument form of `AssetRiskParams` (Borsh)."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "shockBps",
            "type": "u16"
          },
          {
            "name": "gapBps",
            "type": "u16"
          },
          {
            "name": "basisBps",
            "type": "u16"
          }
        ]
      }
    },
    {
      "name": "assetRiskParams",
      "docs": [
        "Per-asset stress parameters as stored. `shock_bps == 0` means unsupported:",
        "refresh fails closed for any leg in that asset."
      ],
      "serialization": "bytemuck",
      "repr": {
        "kind": "c"
      },
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "shockBps",
            "type": "u16"
          },
          {
            "name": "gapBps",
            "type": "u16"
          },
          {
            "name": "basisBps",
            "type": "u16"
          }
        ]
      }
    },
    {
      "name": "changeApplied",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "authority",
            "type": "pubkey"
          }
        ]
      }
    },
    {
      "name": "changeCancelled",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "authority",
            "type": "pubkey"
          }
        ]
      }
    },
    {
      "name": "changeQueued",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "proposer",
            "type": "pubkey"
          },
          {
            "name": "etaSlot",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "configParams",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "maxVenues",
            "docs": [
              "Highest venue index + 1 that may be registered (<= 64)."
            ],
            "type": "u8"
          },
          {
            "name": "haircutBps",
            "docs": [
              "Safety haircut applied to the computed hedge benefit."
            ],
            "type": "u16"
          },
          {
            "name": "creditTtlSlots",
            "docs": [
              "Credit lifetime in slots (1..=MAX_CREDIT_TTL_SLOTS)."
            ],
            "type": "u64"
          },
          {
            "name": "snapshotMaxAgeSlots",
            "docs": [
              "A snapshot older than this cannot back credit."
            ],
            "type": "u64"
          },
          {
            "name": "maxCreditPerUser",
            "docs": [
              "Cap on one portfolio's total credit (micro-USD)."
            ],
            "type": "u64"
          },
          {
            "name": "maxCreditBpsOfIm",
            "docs": [
              "Per-leg cap as a fraction of the leg's initial margin."
            ],
            "type": "u16"
          },
          {
            "name": "maxCreditPerVenue",
            "docs": [
              "Cap on outstanding credit across all users, per venue (micro-USD)."
            ],
            "type": "u64"
          },
          {
            "name": "maxCreditPerAsset",
            "docs": [
              "Cap on outstanding credit across all users, per asset (micro-USD)."
            ],
            "type": "u64"
          },
          {
            "name": "globalCreditCap",
            "docs": [
              "Protocol-wide cap on outstanding credit (micro-USD)."
            ],
            "type": "u64"
          },
          {
            "name": "riskModelMaxAgeSlots",
            "docs": [
              "The risk model must have been updated within this many slots."
            ],
            "type": "u64"
          },
          {
            "name": "maxPriceAgeSecs",
            "type": "i64"
          },
          {
            "name": "maxConfBps",
            "type": "u16"
          },
          {
            "name": "maxMoveBps",
            "docs": [
              "Price move vs the portfolio's stored baseline that allows permissionless invalidation."
            ],
            "type": "u16"
          },
          {
            "name": "timelockSlots",
            "docs": [
              "Delay before a credit-increasing governance change or venue activation",
              "takes effect (floor: governance::MIN_TIMELOCK_SLOTS)."
            ],
            "type": "u64"
          },
          {
            "name": "hedgeBreakBps",
            "docs": [
              "Hedge-break scenario (A3): every leg moves adversely at once by",
              "`shock · hedge_break_bps / 10_000`. A ρ-independent cap on credit.",
              "0 = disabled (allowed only in mock-oracle builds); otherwise",
              "`ch_math::risk::HEDGE_BREAK_MIN_BPS..=HEDGE_BREAK_MAX_BPS`."
            ],
            "type": "u16"
          },
          {
            "name": "maxLegNotional",
            "docs": [
              "A5a: largest notional one report may carry (micro-USD, > 0)."
            ],
            "type": "u64"
          },
          {
            "name": "maxVenueNotionalPerUser",
            "docs": [
              "A5b: largest sum of one user's notionals on one venue (micro-USD, > 0)."
            ],
            "type": "u64"
          },
          {
            "name": "maxNotionalGrowthBps",
            "docs": [
              "A5c: inside a window, a leg's notional may grow to at most",
              "max(base · (1 + g / 10_000), base + floor); 0 = no growth limit."
            ],
            "type": "u16"
          },
          {
            "name": "notionalGrowthWindowSlots",
            "docs": [
              "A5c: window length in slots (1..=216,000)."
            ],
            "type": "u64"
          },
          {
            "name": "notionalGrowthFloor",
            "docs": [
              "A5c: absolute growth always allowed per window (micro-USD), so small",
              "and new positions can open."
            ],
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "consentUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "isActive",
            "type": "bool"
          },
          {
            "name": "authorizedVenuesBitmap",
            "type": "u64"
          },
          {
            "name": "portfolioVersion",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "corrEntry",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "a",
            "type": "u8"
          },
          {
            "name": "b",
            "type": "u8"
          },
          {
            "name": "rho",
            "docs": [
              "Scaled 1e6, in [-1e6, 1e6]."
            ],
            "type": "i32"
          }
        ]
      }
    },
    {
      "name": "emergencyEpochBump",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "authority",
            "type": "pubkey"
          },
          {
            "name": "riskEpoch",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "globalConfig",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "admin",
            "docs": [
              "Role management, unpause, admin transfer. Can be a multisig PDA."
            ],
            "type": "pubkey"
          },
          {
            "name": "proposedAdmin",
            "type": "pubkey"
          },
          {
            "name": "riskAuthority",
            "docs": [
              "Parameters, correlations, shocks, scenarios (bounded, timelocked)."
            ],
            "type": "pubkey"
          },
          {
            "name": "venueManager",
            "docs": [
              "Register, activate (timelocked) and deactivate venues."
            ],
            "type": "pubkey"
          },
          {
            "name": "guardian",
            "docs": [
              "Pause, emergency epoch bump, cancel pending changes, deactivate",
              "venues: powers that can only reduce credit."
            ],
            "type": "pubkey"
          },
          {
            "name": "riskEpoch",
            "docs": [
              "Bumped by every parameter, risk-model and venue-lifecycle change and by",
              "emergency invalidation. A portfolio is usable only while its epoch matches."
            ],
            "type": "u64"
          },
          {
            "name": "params",
            "type": {
              "defined": {
                "name": "configParams"
              }
            }
          },
          {
            "name": "usedVenuesBitmap",
            "docs": [
              "Indices ever assigned. Never cleared: an index is not reusable."
            ],
            "type": "u64"
          },
          {
            "name": "activeVenuesBitmap",
            "docs": [
              "Indices of currently active venues (subset of `used_venues_bitmap`)."
            ],
            "type": "u64"
          },
          {
            "name": "globalOutstandingCredit",
            "docs": [
              "Outstanding credit counted in the current epoch. Reset on every epoch bump."
            ],
            "type": "u64"
          },
          {
            "name": "venueOutstanding",
            "type": {
              "array": [
                "u64",
                64
              ]
            }
          },
          {
            "name": "assetOutstanding",
            "type": {
              "array": [
                "u64",
                32
              ]
            }
          },
          {
            "name": "paused",
            "type": "bool"
          },
          {
            "name": "bump",
            "type": "u8"
          },
          {
            "name": "venueAccepts",
            "docs": [
              "A1: per venue index, the venues whose reports it accepts credit from",
              "(bit j = venue j; its own bit is always implied). Default 0: a venue",
              "accepts only its own reports. Set by the venue (`set_venue_acceptance`)."
            ],
            "type": {
              "array": [
                "u64",
                64
              ]
            }
          }
        ]
      }
    },
    {
      "name": "legKey",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "assetId",
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "legState",
      "docs": [
        "The inputs a leg's credit was computed from, plus the credit itself."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "assetId",
            "type": "u8"
          },
          {
            "name": "isLong",
            "type": "bool"
          },
          {
            "name": "snapshotVersion",
            "type": "u64"
          },
          {
            "name": "snapshotSlot",
            "type": "u64"
          },
          {
            "name": "notional",
            "type": "u64"
          },
          {
            "name": "initialMargin",
            "type": "u64"
          },
          {
            "name": "maintenanceMargin",
            "type": "u64"
          },
          {
            "name": "baselinePrice",
            "docs": [
              "Oracle price and confidence at computation time (the guard baseline, F5)."
            ],
            "type": "u64"
          },
          {
            "name": "baselineConf",
            "type": "u64"
          },
          {
            "name": "baselinePublishTs",
            "docs": [
              "Oracle publish time of the baseline. Invalidation evidence must be",
              "strictly newer, so an old (but genuine) update cannot trip credit."
            ],
            "type": "i64"
          },
          {
            "name": "credit",
            "type": "u64"
          },
          {
            "name": "viewMask",
            "docs": [
              "A1: venues whose legs this credit was computed from (own bit",
              "included). Usable only while ⊆ the venue's current allowlist."
            ],
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "mockPrice",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "assetId",
            "type": "u8"
          },
          {
            "name": "priceMicro",
            "type": "u64"
          },
          {
            "name": "confMicro",
            "type": "u64"
          },
          {
            "name": "publishTs",
            "type": "i64"
          },
          {
            "name": "bump",
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "paramsUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "authority",
            "type": "pubkey"
          },
          {
            "name": "params",
            "type": {
              "defined": {
                "name": "configParams"
              }
            }
          },
          {
            "name": "riskEpoch",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "pausedUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "authority",
            "type": "pubkey"
          },
          {
            "name": "paused",
            "type": "bool"
          }
        ]
      }
    },
    {
      "name": "pendingChange",
      "docs": [
        "Singleton. Seeds: [SEED_PENDING_CHANGE]."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "change",
            "type": {
              "defined": {
                "name": "riskChange"
              }
            }
          },
          {
            "name": "proposer",
            "type": "pubkey"
          },
          {
            "name": "proposedSlot",
            "type": "u64"
          },
          {
            "name": "etaSlot",
            "type": "u64"
          },
          {
            "name": "bump",
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "portfolio",
      "docs": [
        "One per user. Seeds: [SEED_PORTFOLIO, user]."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "status",
            "type": "u8"
          },
          {
            "name": "creditKind",
            "docs": [
              "Credit semantics flag; always CREDIT_KIND_TRADING_POWER_ONLY."
            ],
            "type": "u8"
          },
          {
            "name": "version",
            "docs": [
              "Incremented on every computation and every invalidation."
            ],
            "type": "u64"
          },
          {
            "name": "riskEpoch",
            "docs": [
              "Config epoch / risk-model version the credit was computed under."
            ],
            "type": "u64"
          },
          {
            "name": "riskModelVersion",
            "type": "u64"
          },
          {
            "name": "computedSlot",
            "type": "u64"
          },
          {
            "name": "expiresSlot",
            "type": "u64"
          },
          {
            "name": "totalCredit",
            "type": "u64"
          },
          {
            "name": "counted",
            "docs": [
              "True while `legs[*].credit` / `total_credit` are included in the",
              "config's outstanding counters for epoch `counted_epoch`."
            ],
            "type": "bool"
          },
          {
            "name": "countedEpoch",
            "type": "u64"
          },
          {
            "name": "sumStandaloneRisk",
            "type": "u64"
          },
          {
            "name": "portfolioRisk",
            "type": "u64"
          },
          {
            "name": "hedgeBenefitPpm",
            "type": "u32"
          },
          {
            "name": "flags",
            "docs": [
              "`portfolio_flags` bits describing the last computation (A4, B3)."
            ],
            "type": "u8"
          },
          {
            "name": "numRegistered",
            "docs": [
              "Every (venue, asset) for which a snapshot account exists. A refresh",
              "must include all of them."
            ],
            "type": "u8"
          },
          {
            "name": "registered",
            "type": {
              "array": [
                {
                  "defined": {
                    "name": "legKey"
                  }
                },
                8
              ]
            }
          },
          {
            "name": "registeredNotional",
            "docs": [
              "A5b: notional last reported for each registered key (same order)."
            ],
            "type": {
              "array": [
                "u64",
                8
              ]
            }
          },
          {
            "name": "numLegs",
            "docs": [
              "Legs as of the last computation."
            ],
            "type": "u8"
          },
          {
            "name": "legs",
            "type": {
              "array": [
                {
                  "defined": {
                    "name": "legState"
                  }
                },
                8
              ]
            }
          },
          {
            "name": "bump",
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "portfolioInvalidated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "version",
            "type": "u64"
          },
          {
            "name": "reasons",
            "type": "u16"
          }
        ]
      }
    },
    {
      "name": "portfolioRefreshed",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "version",
            "type": "u64"
          },
          {
            "name": "riskEpoch",
            "type": "u64"
          },
          {
            "name": "numLegs",
            "type": "u8"
          },
          {
            "name": "sumStandaloneRisk",
            "type": "u64"
          },
          {
            "name": "portfolioRisk",
            "type": "u64"
          },
          {
            "name": "hedgeBenefitPpm",
            "type": "u32"
          },
          {
            "name": "totalCredit",
            "type": "u64"
          },
          {
            "name": "expiresSlot",
            "type": "u64"
          },
          {
            "name": "flags",
            "docs": [
              "`state::portfolio_flags` (A4: PSD fallback)."
            ],
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "posSnapshot",
      "docs": [
        "One position of one user on one venue in one asset, as reported by the venue.",
        "Seeds: [SEED_SNAPSHOT, user, venue_id, [asset_id]]."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "venueId",
            "type": {
              "array": [
                "u8",
                32
              ]
            }
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "assetId",
            "type": "u8"
          },
          {
            "name": "version",
            "docs": [
              "Monotonic: 1 on creation, +1 on every write."
            ],
            "type": "u64"
          },
          {
            "name": "isLong",
            "type": "bool"
          },
          {
            "name": "notional",
            "type": "u64"
          },
          {
            "name": "initialMargin",
            "type": "u64"
          },
          {
            "name": "maintenanceMargin",
            "type": "u64"
          },
          {
            "name": "slot",
            "type": "u64"
          },
          {
            "name": "timestamp",
            "type": "i64"
          },
          {
            "name": "rentPayer",
            "docs": [
              "Who paid rent; receives it back when the snapshot is closed."
            ],
            "type": "pubkey"
          },
          {
            "name": "bump",
            "type": "u8"
          },
          {
            "name": "windowStartSlot",
            "docs": [
              "A5c: growth window start and the notional at its start."
            ],
            "type": "u64"
          },
          {
            "name": "windowBaseNotional",
            "type": "u64"
          },
          {
            "name": "markPrice",
            "docs": [
              "A5d: venue-reported mark price (micro-USD), 0 = not reported."
            ],
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "positionSnapshotClosed",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "assetId",
            "type": "u8"
          },
          {
            "name": "portfolioVersion",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "positionSnapshotSubmitted",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "assetId",
            "type": "u8"
          },
          {
            "name": "version",
            "type": "u64"
          },
          {
            "name": "notional",
            "type": "u64"
          },
          {
            "name": "isLong",
            "type": "bool"
          },
          {
            "name": "initialMargin",
            "type": "u64"
          },
          {
            "name": "maintenanceMargin",
            "type": "u64"
          },
          {
            "name": "slot",
            "type": "u64"
          },
          {
            "name": "portfolioVersion",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "riskChange",
      "docs": [
        "A credit-increasing governance change waiting for its timelock."
      ],
      "type": {
        "kind": "enum",
        "variants": [
          {
            "name": "none"
          },
          {
            "name": "params",
            "fields": [
              {
                "defined": {
                  "name": "configParams"
                }
              }
            ]
          },
          {
            "name": "correlations",
            "fields": [
              {
                "vec": {
                  "defined": {
                    "name": "corrEntry"
                  }
                }
              }
            ]
          },
          {
            "name": "assetRisk",
            "fields": [
              {
                "name": "assetId",
                "type": "u8"
              },
              {
                "name": "params",
                "type": {
                  "defined": {
                    "name": "assetRiskArgs"
                  }
                }
              }
            ]
          },
          {
            "name": "stress",
            "fields": [
              {
                "name": "corrStressPpm",
                "type": "u32"
              },
              {
                "name": "basisDecorrPpm",
                "type": "u32"
              }
            ]
          },
          {
            "name": "scenario",
            "fields": [
              {
                "name": "index",
                "type": "u8"
              },
              {
                "name": "kind",
                "type": "u8"
              },
              {
                "name": "moves",
                "type": {
                  "vec": {
                    "defined": {
                      "name": "assetMove"
                    }
                  }
                }
              }
            ]
          },
          {
            "name": "oracleFeed",
            "fields": [
              {
                "name": "assetId",
                "type": "u8"
              },
              {
                "name": "feedId",
                "type": {
                  "array": [
                    "u8",
                    32
                  ]
                }
              }
            ]
          }
        ]
      }
    },
    {
      "name": "riskModel",
      "docs": [
        "Zero-copy: at ~2.7 KiB a Borsh copy of this account does not fit in one",
        "4 KiB SBF stack frame, so it is read and written in place."
      ],
      "serialization": "bytemuck",
      "repr": {
        "kind": "c"
      },
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "version",
            "docs": [
              "Incremented on every change."
            ],
            "type": "u64"
          },
          {
            "name": "updatedSlot",
            "type": "u64"
          },
          {
            "name": "corr",
            "docs": [
              "Upper-triangle asset correlations, scaled 1e6, in [-1e6, 1e6]."
            ],
            "type": {
              "array": [
                "i32",
                496
              ]
            }
          },
          {
            "name": "assets",
            "type": {
              "array": [
                {
                  "defined": {
                    "name": "assetRiskParams"
                  }
                },
                32
              ]
            }
          },
          {
            "name": "corrStressPpm",
            "docs": [
              "Correlation stress delta, scaled 1e6."
            ],
            "type": "u32"
          },
          {
            "name": "basisDecorrPpm",
            "docs": [
              "Decorrelation of offsetting same-asset legs on different venues, 1e6."
            ],
            "type": "u32"
          },
          {
            "name": "scenarioKind",
            "docs": [
              "`ch_math::risk::scenario_kind` per slot (0 = unused)."
            ],
            "type": {
              "array": [
                "u8",
                8
              ]
            }
          },
          {
            "name": "scenarios",
            "docs": [
              "Per-asset signed moves in bps; an unused slot is all zeros (no loss)."
            ],
            "type": {
              "array": [
                {
                  "array": [
                    "i16",
                    32
                  ]
                },
                8
              ]
            }
          },
          {
            "name": "oracleFeeds",
            "docs": [
              "Pyth feed id per asset (`pyth` builds); all zeros = no feed, and",
              "every price read for that asset fails closed."
            ],
            "type": {
              "array": [
                {
                  "array": [
                    "u8",
                    32
                  ]
                },
                32
              ]
            }
          },
          {
            "name": "bump",
            "type": "u8"
          },
          {
            "name": "padding",
            "type": {
              "array": [
                "u8",
                7
              ]
            }
          }
        ]
      }
    },
    {
      "name": "riskModelUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "version",
            "type": "u64"
          },
          {
            "name": "entries",
            "type": "u16"
          },
          {
            "name": "riskEpoch",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "rolesUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "admin",
            "type": "pubkey"
          },
          {
            "name": "riskAuthority",
            "type": "pubkey"
          },
          {
            "name": "venueManager",
            "type": "pubkey"
          },
          {
            "name": "guardian",
            "type": "pubkey"
          }
        ]
      }
    },
    {
      "name": "userAccountsClosed",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "portfolioVersion",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "userConsent",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "user",
            "type": "pubkey"
          },
          {
            "name": "isActive",
            "type": "bool"
          },
          {
            "name": "authorizedVenuesBitmap",
            "type": "u64"
          },
          {
            "name": "bump",
            "type": "u8"
          }
        ]
      }
    },
    {
      "name": "venueAcceptanceUpdated",
      "docs": [
        "A1: a venue changed which venues' reports it accepts credit from."
      ],
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "acceptsMask",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "venueActiveUpdated",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueId",
            "type": {
              "array": [
                "u8",
                32
              ]
            }
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "isActive",
            "type": "bool"
          },
          {
            "name": "activationEtaSlot",
            "type": "u64"
          },
          {
            "name": "riskEpoch",
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "venueRegistered",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueId",
            "type": {
              "array": [
                "u8",
                32
              ]
            }
          },
          {
            "name": "venueProgramId",
            "type": "pubkey"
          },
          {
            "name": "venueAuthority",
            "type": "pubkey"
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "activationEtaSlot",
            "docs": [
              "0 when active immediately (timelock 0)."
            ],
            "type": "u64"
          }
        ]
      }
    },
    {
      "name": "venueRegistration",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "venueId",
            "type": {
              "array": [
                "u8",
                32
              ]
            }
          },
          {
            "name": "venueProgramId",
            "type": "pubkey"
          },
          {
            "name": "venueAuthority",
            "docs": [
              "The key that must sign snapshot writes. In production builds this is",
              "the PDA [b\"clearinghouse\"] of `venue_program_id`, so only that",
              "program (via CPI) can sign."
            ],
            "type": "pubkey"
          },
          {
            "name": "venueIndex",
            "type": "u8"
          },
          {
            "name": "isActive",
            "type": "bool"
          },
          {
            "name": "activationEtaSlot",
            "docs": [
              "Slot from which a requested activation may be completed; 0 = none."
            ],
            "type": "u64"
          },
          {
            "name": "bump",
            "type": "u8"
          }
        ]
      }
    }
  ]
};
