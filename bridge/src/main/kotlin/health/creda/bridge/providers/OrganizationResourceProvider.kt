package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.server.IResourceProvider
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.Organization
import org.springframework.stereotype.Component

/**
 * `GET /Organization` — the institutions known to this peer: the distinct audience names that
 * appear in AuthorizationGrants across the local store (Core's `ListInstitutions`). A read-only
 * *discovery* surface so a client can offer "share with an institution already on the network"
 * without hardcoding a list.
 *
 * Deliberately minimal: Creda models an institution as an identity/certificate fingerprint, not a
 * full directory entry. Only `Organization.name` is populated; richer Organization data is a
 * Participant Registry concern (Appendix C), not the substrate's. Translator-not-reasoner
 * (§8.3.2): this only maps Core's string list to FHIR.
 */
@Component
class OrganizationResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Organization> = Organization::class.java

    /** `GET /Organization` — every institution audience seen in the local store, as Organizations. */
    @Search
    fun all(): List<Organization> =
        core.listInstitutions().map { institutionName ->
            Organization().apply {
                name = institutionName
                // FHIR requires a logical id; derive a stable one from the name (we have no
                // separate institution identifier at this layer). Constrained to the FHIR id
                // charset by using the unsigned hash.
                id = institutionName.hashCode().toUInt().toString()
            }
        }
}
