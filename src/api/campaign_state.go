package api

import (
	"encoding/json"
)

// GetCampaignState returns campaign state - compatibility wrapper
func (a *API) GetCampaignState() CampaignState {
	entitiesMap := a.ran.Campaign.GetEntities()
	entities := make(map[string]map[string]interface{}, len(entitiesMap))
	for id, entity := range entitiesMap {
		// Convert entity to map for JSON compatibility
		data, _ := json.Marshal(entity)
		var entityMap map[string]interface{}
		json.Unmarshal(data, &entityMap)
		entities[id] = entityMap
	}

	relationsMap := a.ran.Campaign.GetRelations()
	relations := make([]map[string]interface{}, 0, len(relationsMap))
	for _, relation := range relationsMap {
		// Convert relation to map for JSON compatibility
		data, _ := json.Marshal(relation)
		var relationMap map[string]interface{}
		json.Unmarshal(data, &relationMap)
		relations = append(relations, relationMap)
	}

	return CampaignState{
		Entities:  entities,
		Relations: relations,
	}
}
