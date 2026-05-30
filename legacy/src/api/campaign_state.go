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
	for id, relation := range relationsMap {
		// Convert relation to map for JSON compatibility
		data, _ := json.Marshal(relation)
		var relationMap map[string]interface{}
		json.Unmarshal(data, &relationMap)
		// Add the relation ID to ensure frontend can look it up
		relationMap["id"] = id
		// Add source and target IDs for convenience
		relationMap["source"] = relation.GetSourceId()
		relationMap["target"] = relation.GetTargetId()
		relationMap["kind"] = relation.GetRelationName()
		relations = append(relations, relationMap)
	}

	return CampaignState{
		Entities:  entities,
		Relations: relations,
	}
}
