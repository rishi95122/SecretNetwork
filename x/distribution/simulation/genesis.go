package simulation

// DONTCOVER

import (
	"encoding/json"
	"fmt"
	"math/rand"

	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/module"
	"github.com/cosmos/cosmos-sdk/types/simulation"
	"github.com/enigmampc/SecretNetwork/x/distribution/types"
)

// Simulation parameter constants
const (
	CommunityTax        = "community_tax"
	BaseProposerReward  = "base_proposer_reward"
	BonusProposerReward = "bonus_proposer_reward"
	WithdrawEnabled     = "withdraw_enabled"
	FoundationTax       = "foundation_tax"
)

// GenCommunityTax randomized CommunityTax
func GenCommunityTax(r *rand.Rand) sdk.Dec {
	return sdk.NewDecWithPrec(1, 2).Add(sdk.NewDecWithPrec(int64(r.Intn(30)), 2))
}

// GenBaseProposerReward randomized BaseProposerReward
func GenBaseProposerReward(r *rand.Rand) sdk.Dec {
	return sdk.NewDecWithPrec(1, 2).Add(sdk.NewDecWithPrec(int64(r.Intn(30)), 2))
}

// GenBonusProposerReward randomized BonusProposerReward
func GenBonusProposerReward(r *rand.Rand) sdk.Dec {
	return sdk.NewDecWithPrec(1, 2).Add(sdk.NewDecWithPrec(int64(r.Intn(30)), 2))
}

// GenWithdrawEnabled returns a randomized WithdrawEnabled parameter.
func GenWithdrawEnabled(r *rand.Rand) bool {
	return r.Int63n(101) <= 95 // 95% chance of withdraws being enabled
}

// GenSecretFoundationTax returns a randomized secret foundation tax parameter.
func GenSecretFoundationTax(r *rand.Rand) sdk.Dec {
	return sdk.NewDecWithPrec(1, 2).Add(sdk.NewDecWithPrec(int64(r.Intn(30)), 2))
}

// RandomizedGenState generates a random GenesisState for distribution
func RandomizedGenState(simState *module.SimulationState) {
	var communityTax sdk.Dec
	simState.AppParams.GetOrGenerate(
		simState.Cdc, CommunityTax, &communityTax, simState.Rand,
		func(r *rand.Rand) { communityTax = GenCommunityTax(r) },
	)

	var baseProposerReward sdk.Dec
	simState.AppParams.GetOrGenerate(
		simState.Cdc, BaseProposerReward, &baseProposerReward, simState.Rand,
		func(r *rand.Rand) { baseProposerReward = GenBaseProposerReward(r) },
	)

	var bonusProposerReward sdk.Dec
	simState.AppParams.GetOrGenerate(
		simState.Cdc, BonusProposerReward, &bonusProposerReward, simState.Rand,
		func(r *rand.Rand) { bonusProposerReward = GenBonusProposerReward(r) },
	)

	var withdrawEnabled bool
	simState.AppParams.GetOrGenerate(
		simState.Cdc, WithdrawEnabled, &withdrawEnabled, simState.Rand,
		func(r *rand.Rand) { withdrawEnabled = GenWithdrawEnabled(r) },
	)

	var foundationTax sdk.Dec
	simState.AppParams.GetOrGenerate(
		simState.Cdc, FoundationTax, &foundationTax, simState.Rand,
		func(r *rand.Rand) { foundationTax = GenSecretFoundationTax(r) },
	)

	foundationTaxAcc, _ := simulation.RandomAcc(simState.Rand, simState.Accounts)

	distrGenesis := types.GenesisState{
		FeePool: types.InitialFeePool(),
		Params: types.Params{
			CommunityTax:            communityTax,
			SecretFoundationTax:     foundationTax,
			SecretFoundationAddress: foundationTaxAcc.Address,
			BaseProposerReward:      baseProposerReward,
			BonusProposerReward:     bonusProposerReward,
			WithdrawAddrEnabled:     withdrawEnabled,
		},
	}

	bz, err := json.MarshalIndent(&distrGenesis, "", " ")
	if err != nil {
		panic(err)
	}
	fmt.Printf("Selected randomly generated distribution parameters:\n%s\n", bz)
	simState.GenState[types.ModuleName] = simState.Cdc.MustMarshalJSON(&distrGenesis)
}